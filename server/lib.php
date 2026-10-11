<?php
// Servidor de cuentas de Kutral: base de datos, esquema y utilidades.
//
// Sin frameworks ni Composer: PHP 8.1+ con pdo_sqlite. Kutral habla con esto
// por JSON; no hay frontend. Ver README.md para instalarlo.

declare(strict_types=1);

const VERSION_API = 1;

function config(): array
{
    static $cfg = null;
    if ($cfg === null) {
        $base = [
            'db' => __DIR__ . '/datos/kutral.sqlite',
            // Intentos de login fallidos por IP antes de frenar, y ventana.
            'intentos_max' => 10,
            'intentos_ventana' => 900,
        ];
        $archivo = __DIR__ . '/config.php';
        $cfg = is_file($archivo) ? array_merge($base, require $archivo) : $base;
    }
    return $cfg;
}

function db(): PDO
{
    static $pdo = null;
    if ($pdo !== null) {
        return $pdo;
    }
    $ruta = config()['db'];
    $dir = dirname($ruta);
    if (!is_dir($dir)) {
        mkdir($dir, 0700, true);
    }
    $pdo = new PDO('sqlite:' . $ruta, null, null, [
        PDO::ATTR_ERRMODE => PDO::ERRMODE_EXCEPTION,
        PDO::ATTR_DEFAULT_FETCH_MODE => PDO::FETCH_ASSOC,
    ]);
    // WAL: lecturas de un equipo no bloquean la escritura de otro.
    $pdo->exec('PRAGMA journal_mode = WAL');
    $pdo->exec('PRAGMA foreign_keys = ON');
    $pdo->exec('PRAGMA busy_timeout = 5000');
    migrar($pdo);
    return $pdo;
}

// Migraciones por `user_version`: cada entrada se aplica una sola vez, en orden.
function migrar(PDO $pdo): void
{
    $migraciones = [
        1 => "
            CREATE TABLE usuarios (
                id INTEGER PRIMARY KEY,
                nombre TEXT NOT NULL UNIQUE,
                pass_hash TEXT NOT NULL,
                creado INTEGER NOT NULL
            );

            CREATE TABLE invitaciones (
                codigo TEXT PRIMARY KEY,
                creada INTEGER NOT NULL,
                usada_por INTEGER REFERENCES usuarios(id) ON DELETE SET NULL,
                usada_en INTEGER
            );

            -- Un token por equipo: cerrar sesión en uno no afecta a los demás.
            -- Solo se guarda el sha256; el token en claro lo tiene el equipo.
            CREATE TABLE equipos (
                id INTEGER PRIMARY KEY,
                usuario_id INTEGER NOT NULL REFERENCES usuarios(id) ON DELETE CASCADE,
                nombre TEXT NOT NULL,
                token_hash TEXT NOT NULL UNIQUE,
                creado INTEGER NOT NULL,
                ultimo_uso INTEGER NOT NULL
            );

            CREATE TABLE perfiles (
                id INTEGER PRIMARY KEY,
                usuario_id INTEGER NOT NULL REFERENCES usuarios(id) ON DELETE CASCADE,
                nombre TEXT NOT NULL,
                creado INTEGER NOT NULL
            );

            -- Todo lo sincronizable de un perfil (favoritos, historial, ...).
            -- `seq` crece en cada cambio aceptado: es el cursor de GET /sync.
            -- Un borrado queda como lápida (borrado = 1, datos NULL) para que
            -- los demás equipos se enteren.
            CREATE TABLE registros (
                perfil_id INTEGER NOT NULL REFERENCES perfiles(id) ON DELETE CASCADE,
                coleccion TEXT NOT NULL,
                clave TEXT NOT NULL,
                datos TEXT,
                borrado INTEGER NOT NULL DEFAULT 0,
                actualizado INTEGER NOT NULL,
                seq INTEGER NOT NULL,
                PRIMARY KEY (perfil_id, coleccion, clave)
            );
            CREATE INDEX idx_reg_seq ON registros(perfil_id, seq);

            CREATE TABLE secuencia (n INTEGER NOT NULL);
            INSERT INTO secuencia (n) VALUES (0);

            CREATE TABLE intentos (
                ip TEXT NOT NULL,
                momento INTEGER NOT NULL
            );
            CREATE INDEX idx_intentos ON intentos(ip, momento);
        ",
    ];
    $actual = (int) $pdo->query('PRAGMA user_version')->fetchColumn();
    foreach ($migraciones as $version => $sql) {
        if ($version <= $actual) {
            continue;
        }
        $pdo->beginTransaction();
        $pdo->exec($sql);
        $pdo->exec('PRAGMA user_version = ' . $version);
        $pdo->commit();
    }
}

function ahora_ms(): int
{
    return (int) floor(microtime(true) * 1000);
}

function siguiente_seq(PDO $pdo): int
{
    $pdo->exec('UPDATE secuencia SET n = n + 1');
    return (int) $pdo->query('SELECT n FROM secuencia')->fetchColumn();
}

function nombre_valido(string $nombre): bool
{
    return (bool) preg_match('/^[a-z0-9_.-]{3,32}$/', $nombre);
}

function crear_usuario(PDO $pdo, string $nombre, string $clave): int
{
    $st = $pdo->prepare('INSERT INTO usuarios (nombre, pass_hash, creado) VALUES (?, ?, ?)');
    $st->execute([$nombre, password_hash($clave, PASSWORD_DEFAULT), ahora_ms()]);
    $id = (int) $pdo->lastInsertId();
    // Todo usuario nace con un perfil: el sync siempre es por perfil.
    $pdo->prepare('INSERT INTO perfiles (usuario_id, nombre, creado) VALUES (?, ?, ?)')
        ->execute([$id, $nombre, ahora_ms()]);
    return $id;
}

function nuevo_codigo(): string
{
    // Sin 0/O/1/I: se dicta por teléfono sin confundirse.
    $abc = 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789';
    $s = '';
    for ($i = 0; $i < 10; $i++) {
        $s .= $abc[random_int(0, strlen($abc) - 1)];
    }
    return substr($s, 0, 5) . '-' . substr($s, 5);
}

// Largo en caracteres sin exigir mbstring (no viene en todas las instalaciones).
function largo(string $s): int
{
    return preg_match_all('/./su', $s) ?: strlen($s);
}
