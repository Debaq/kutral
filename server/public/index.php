<?php
// API JSON de cuentas de Kutral. Único punto de entrada; /admin es el panel
// web del administrador (panel.php).
//
// Funciona con o sin reescritura de URLs:
//   https://servidor/kutral/sync            (con rewrite, ver README.md)
//   https://servidor/kutral/index.php/sync  (sin rewrite, vía PATH_INFO)

declare(strict_types=1);

require __DIR__ . '/../lib.php';

final class Fallo extends Exception
{
    /** @var int */
    public $estado;

    public function __construct(int $estado, string $mensaje)
    {
        parent::__construct($mensaje);
        $this->estado = $estado;
    }
}

function responder(int $estado, array $cuerpo): void
{
    http_response_code($estado);
    header('Content-Type: application/json; charset=utf-8');
    header('Cache-Control: no-store');
    echo json_encode($cuerpo, JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES);
    exit;
}

function ruta(): string
{
    if (!empty($_SERVER['PATH_INFO'])) {
        return rtrim($_SERVER['PATH_INFO'], '/') ?: '/';
    }
    $path = parse_url($_SERVER['REQUEST_URI'] ?? '/', PHP_URL_PATH) ?: '/';
    $base = rtrim(str_replace('\\', '/', dirname($_SERVER['SCRIPT_NAME'] ?? '')), '/');
    // Con server/.htaccess el script es /kutral/public/index.php pero la URL
    // pedida es /kutral/admin: la base es la carpeta de arriba.
    if (str_ends_with($base, '/public') && !str_starts_with($path, $base . '/')) {
        $base = substr($base, 0, -strlen('/public'));
    }
    if ($base !== '' && str_starts_with($path, $base)) {
        $path = substr($path, strlen($base));
    }
    if (str_starts_with($path, '/index.php')) {
        $path = substr($path, strlen('/index.php'));
    }
    return rtrim($path, '/') ?: '/';
}

function cuerpo(): array
{
    $raw = file_get_contents('php://input', false, null, 0, 4 * 1024 * 1024);
    if ($raw === false || $raw === '') {
        return [];
    }
    $json = json_decode($raw, true);
    if (!is_array($json)) {
        throw new Fallo(400, 'JSON inválido');
    }
    return $json;
}

function texto(array $b, string $campo, int $max = 200): string
{
    $v = $b[$campo] ?? null;
    if (!is_string($v) || $v === '' || largo($v) > $max) {
        throw new Fallo(400, "falta o sobra '$campo'");
    }
    return $v;
}

function ip(): string
{
    return $_SERVER['REMOTE_ADDR'] ?? '?';
}

// Freno contra adivinar contraseñas: N fallos por IP en la ventana → 429.
function frenar_si_abusa(PDO $pdo): void
{
    $cfg = config();
    $desde = time() - $cfg['intentos_ventana'];
    $pdo->prepare('DELETE FROM intentos WHERE momento < ?')->execute([$desde]);
    $st = $pdo->prepare('SELECT COUNT(*) FROM intentos WHERE ip = ? AND momento >= ?');
    $st->execute([ip(), $desde]);
    if ((int) $st->fetchColumn() >= $cfg['intentos_max']) {
        throw new Fallo(429, 'demasiados intentos, espera unos minutos');
    }
}

function anotar_fallo(PDO $pdo): void
{
    $pdo->prepare('INSERT INTO intentos (ip, momento) VALUES (?, ?)')->execute([ip(), time()]);
}

function emitir_token(PDO $pdo, int $usuario, string $equipo): string
{
    $token = bin2hex(random_bytes(32));
    $t = ahora_ms();
    $pdo->prepare(
        'INSERT INTO equipos (usuario_id, nombre, token_hash, creado, ultimo_uso) VALUES (?, ?, ?, ?, ?)'
    )->execute([$usuario, $equipo, hash('sha256', $token), $t, $t]);
    return $token;
}

/** @return array{usuario:int, equipo:int} */
function autenticar(PDO $pdo): array
{
    $h = $_SERVER['HTTP_AUTHORIZATION'] ?? $_SERVER['REDIRECT_HTTP_AUTHORIZATION'] ?? '';
    if (!preg_match('/^Bearer\s+([0-9a-f]{64})$/', $h, $m)) {
        throw new Fallo(401, 'sin sesión');
    }
    $st = $pdo->prepare('SELECT id, usuario_id FROM equipos WHERE token_hash = ?');
    $st->execute([hash('sha256', $m[1])]);
    $fila = $st->fetch();
    if (!$fila) {
        throw new Fallo(401, 'sesión cerrada');
    }
    $pdo->prepare('UPDATE equipos SET ultimo_uso = ? WHERE id = ?')->execute([ahora_ms(), $fila['id']]);
    return ['usuario' => (int) $fila['usuario_id'], 'equipo' => (int) $fila['id']];
}

function perfil_propio(PDO $pdo, int $usuario, $perfil): int
{
    $st = $pdo->prepare('SELECT id FROM perfiles WHERE id = ? AND usuario_id = ?');
    $st->execute([(int) $perfil, $usuario]);
    $id = $st->fetchColumn();
    if ($id === false) {
        throw new Fallo(404, 'perfil inexistente');
    }
    return (int) $id;
}

function sesion(PDO $pdo, int $usuario, string $token): array
{
    $st = $pdo->prepare('SELECT nombre FROM usuarios WHERE id = ?');
    $st->execute([$usuario]);
    return [
        'token' => $token,
        'usuario' => $st->fetchColumn(),
        'perfiles' => perfiles($pdo, $usuario),
    ];
}

function perfiles(PDO $pdo, int $usuario): array
{
    $st = $pdo->prepare('SELECT * FROM perfiles WHERE usuario_id = ? ORDER BY id');
    $st->execute([$usuario]);
    return array_map('fila_perfil', $st->fetchAll());
}

function fila_perfil(array $p): array
{
    return [
        'id' => (int) $p['id'],
        'uid' => $p['uid'],
        'nombre' => $p['nombre'],
        'color' => $p['color'],
        'avatar' => $p['avatar'],
        'infantil' => (bool) $p['infantil'],
        'actualizado' => (int) $p['actualizado'],
    ];
}

const MAX_PERFILES = 10;

// Crea el perfil o, si ya existe (mismo uid), le aplica el cambio (nombre,
// color, carita, infantil) solo si es más nuevo que el que tiene. Un equipo
// viejo no manda carita ni infantil: se quedan los que había.
function guardar_perfil(PDO $pdo, array $s, array $b): array
{
    $uid = texto($b, 'uid', 64);
    $nombre = trim(texto($b, 'nombre', 40));
    $color = $b['color'] ?? '#f5c518';
    $act = $b['actualizado'] ?? null;
    $avatar = $b['avatar'] ?? null;
    $infantil = $b['infantil'] ?? null;
    if (!preg_match('/^[A-Za-z0-9_-]{1,64}$/', $uid) || $nombre === ''
        || !is_string($color) || !preg_match('/^#[0-9a-fA-F]{6}$/', $color) || !is_int($act)
        || ($avatar !== null && (!is_string($avatar) || strlen($avatar) > 300
            || ($avatar !== '' && !preg_match('#^https://[^\s"\'<>]+$#', $avatar))))
        || ($infantil !== null && !is_bool($infantil))) {
        throw new Fallo(400, 'perfil inválido');
    }
    $st = $pdo->prepare('SELECT * FROM perfiles WHERE usuario_id = ? AND uid = ?');
    $st->execute([$s['usuario'], $uid]);
    $p = $st->fetch();
    if ($p) {
        if ($act > (int) $p['actualizado']) {
            $avatar = $avatar ?? $p['avatar'];
            $infantil = $infantil ?? (bool) $p['infantil'];
            $pdo->prepare('UPDATE perfiles SET nombre = ?, color = ?, avatar = ?, infantil = ?, actualizado = ? WHERE id = ?')
                ->execute([$nombre, $color, $avatar, $infantil ? 1 : 0, $act, $p['id']]);
        }
    } else {
        $n = $pdo->prepare('SELECT COUNT(*) FROM perfiles WHERE usuario_id = ?');
        $n->execute([$s['usuario']]);
        if ((int) $n->fetchColumn() >= MAX_PERFILES) {
            throw new Fallo(409, 'la cuenta ya tiene ' . MAX_PERFILES . ' perfiles');
        }
        $pdo->prepare(
            'INSERT INTO perfiles (usuario_id, nombre, creado, uid, color, avatar, infantil, actualizado)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)'
        )->execute([$s['usuario'], $nombre, ahora_ms(), $uid, $color, $avatar ?? '', $infantil ? 1 : 0, $act]);
    }
    $st->execute([$s['usuario'], $uid]);
    return fila_perfil($st->fetch());
}

// Borra el perfil y todo lo sincronizado de él. El principal no se borra.
function borrar_perfil(PDO $pdo, array $s, int $id): array
{
    $st = $pdo->prepare('SELECT uid FROM perfiles WHERE id = ? AND usuario_id = ?');
    $st->execute([$id, $s['usuario']]);
    $uid = $st->fetchColumn();
    if ($uid === 'principal') {
        throw new Fallo(400, 'el perfil principal no se puede borrar');
    }
    $pdo->prepare('DELETE FROM perfiles WHERE id = ? AND usuario_id = ?')->execute([$id, $s['usuario']]);
    return ['perfiles' => perfiles($pdo, $s['usuario'])];
}

function fila_registro(array $r): array
{
    return [
        'coleccion' => $r['coleccion'],
        'clave' => $r['clave'],
        'datos' => $r['datos'] === null ? null : json_decode($r['datos'], true),
        'borrado' => (bool) $r['borrado'],
        'actualizado' => (int) $r['actualizado'],
        'seq' => (int) $r['seq'],
    ];
}

// ─── Rutas ──────────────────────────────────────────────────────────────────

function ping(): array
{
    return ['servicio' => 'kutral-cuentas', 'version' => VERSION_API];
}

function registro(PDO $pdo, array $b): array
{
    frenar_si_abusa($pdo);
    $codigo = strtoupper(trim(texto($b, 'invitacion', 20)));
    $nombre = strtolower(trim(texto($b, 'nombre', 32)));
    $clave = texto($b, 'clave', 200);
    $equipo = texto($b, 'equipo', 64);
    if (!nombre_valido($nombre)) {
        throw new Fallo(400, 'el nombre va de 3 a 32 caracteres: letras, números, punto, guion o guion bajo');
    }
    if (largo($clave) < 8) {
        throw new Fallo(400, 'la contraseña necesita al menos 8 caracteres');
    }
    $pdo->exec('BEGIN IMMEDIATE');
    try {
        $st = $pdo->prepare('SELECT 1 FROM invitaciones WHERE codigo = ? AND usada_en IS NULL');
        $st->execute([$codigo]);
        if (!$st->fetchColumn()) {
            throw new Fallo(403, 'invitación inválida o ya usada');
        }
        $st = $pdo->prepare('SELECT 1 FROM usuarios WHERE nombre = ?');
        $st->execute([$nombre]);
        if ($st->fetchColumn()) {
            throw new Fallo(409, 'ese nombre ya existe');
        }
        $id = crear_usuario($pdo, $nombre, $clave);
        $pdo->prepare('UPDATE invitaciones SET usada_por = ?, usada_en = ? WHERE codigo = ?')
            ->execute([$id, ahora_ms(), $codigo]);
        $token = emitir_token($pdo, $id, $equipo);
        $pdo->exec('COMMIT');
    } catch (Throwable $e) {
        $pdo->exec('ROLLBACK');
        // El fallo de invitación se anota fuera de la transacción revertida.
        if ($e instanceof Fallo && $e->estado === 403) {
            anotar_fallo($pdo);
        }
        throw $e;
    }
    return sesion($pdo, $id, $token);
}

function login(PDO $pdo, array $b): array
{
    frenar_si_abusa($pdo);
    $nombre = strtolower(trim(texto($b, 'nombre', 32)));
    $clave = texto($b, 'clave', 200);
    $equipo = texto($b, 'equipo', 64);
    $st = $pdo->prepare('SELECT id, pass_hash FROM usuarios WHERE nombre = ?');
    $st->execute([$nombre]);
    $u = $st->fetch();
    if (!$u || !password_verify($clave, $u['pass_hash'])) {
        anotar_fallo($pdo);
        throw new Fallo(401, 'nombre o contraseña incorrectos');
    }
    if (password_needs_rehash($u['pass_hash'], PASSWORD_DEFAULT)) {
        $pdo->prepare('UPDATE usuarios SET pass_hash = ? WHERE id = ?')
            ->execute([password_hash($clave, PASSWORD_DEFAULT), $u['id']]);
    }
    return sesion($pdo, (int) $u['id'], emitir_token($pdo, (int) $u['id'], $equipo));
}

function yo(PDO $pdo, array $s): array
{
    $st = $pdo->prepare('SELECT nombre FROM usuarios WHERE id = ?');
    $st->execute([$s['usuario']]);
    $nombre = $st->fetchColumn();
    $st = $pdo->prepare('SELECT id, nombre, creado, ultimo_uso FROM equipos WHERE usuario_id = ? ORDER BY ultimo_uso DESC');
    $st->execute([$s['usuario']]);
    $equipos = array_map(fn($e) => [
        'id' => (int) $e['id'],
        'nombre' => $e['nombre'],
        'creado' => (int) $e['creado'],
        'ultimo_uso' => (int) $e['ultimo_uso'],
        'este' => (int) $e['id'] === $s['equipo'],
    ], $st->fetchAll());
    return ['usuario' => $nombre, 'perfiles' => perfiles($pdo, $s['usuario']), 'equipos' => $equipos];
}

// Un equipo que entró antes de la bóveda pide la contraseña para sacar su
// clave; aquí solo se confirma que es la correcta.
function verificar_clave(PDO $pdo, array $s, array $b): array
{
    frenar_si_abusa($pdo);
    $clave = texto($b, 'clave', 200);
    $st = $pdo->prepare('SELECT pass_hash FROM usuarios WHERE id = ?');
    $st->execute([$s['usuario']]);
    if (!password_verify($clave, (string) $st->fetchColumn())) {
        anotar_fallo($pdo);
        throw new Fallo(403, 'contraseña incorrecta');
    }
    return ['ok' => true];
}

// ─── Bóveda ─────────────────────────────────────────────────────────────────

const BOVEDA_MAX = 256 * 1024;

function boveda(PDO $pdo, array $s): array
{
    $st = $pdo->prepare('SELECT datos, version, actualizada FROM bovedas WHERE usuario_id = ?');
    $st->execute([$s['usuario']]);
    $f = $st->fetch();
    if (!$f) {
        return ['version' => 0, 'datos' => null, 'actualizada' => 0];
    }
    return ['version' => (int) $f['version'], 'datos' => $f['datos'], 'actualizada' => (int) $f['actualizada']];
}

// Escribe solo si nadie la cambió desde la versión que el equipo conoce
// (`base`); si no, 409 y el equipo la vuelve a bajar y mezclar.
function guardar_boveda(PDO $pdo, array $s, array $b): array
{
    $datos = texto($b, 'datos', BOVEDA_MAX);
    $base = $b['base'] ?? null;
    if (!is_int($base) || $base < 0) {
        throw new Fallo(400, "falta o sobra 'base'");
    }
    $pdo->exec('BEGIN IMMEDIATE');
    try {
        $st = $pdo->prepare('SELECT version FROM bovedas WHERE usuario_id = ?');
        $st->execute([$s['usuario']]);
        $actual = (int) $st->fetchColumn();
        if ($actual !== $base) {
            throw new Fallo(409, 'la bóveda cambió desde otro equipo');
        }
        $nueva = $actual + 1;
        $pdo->prepare(
            'INSERT OR REPLACE INTO bovedas (usuario_id, datos, version, actualizada) VALUES (?, ?, ?, ?)'
        )->execute([$s['usuario'], $datos, $nueva, ahora_ms()]);
        $pdo->exec('COMMIT');
    } catch (Throwable $e) {
        $pdo->exec('ROLLBACK');
        throw $e;
    }
    return ['version' => $nueva];
}

function quitar_equipo(PDO $pdo, array $s, int $id): array
{
    $pdo->prepare('DELETE FROM equipos WHERE id = ? AND usuario_id = ?')->execute([$id, $s['usuario']]);
    return ['ok' => true];
}

const LOTE = 500;

function bajar(PDO $pdo, array $s): array
{
    $perfil = perfil_propio($pdo, $s['usuario'], $_GET['perfil'] ?? 0);
    $desde = max(0, (int) ($_GET['desde'] ?? 0));
    $st = $pdo->prepare(
        'SELECT coleccion, clave, datos, borrado, actualizado, seq FROM registros
          WHERE perfil_id = ? AND seq > ? ORDER BY seq LIMIT ' . LOTE
    );
    $st->execute([$perfil, $desde]);
    $cambios = array_map('fila_registro', $st->fetchAll());
    return [
        'cambios' => $cambios,
        'hasta' => $cambios ? end($cambios)['seq'] : $desde,
        'mas' => count($cambios) === LOTE,
    ];
}

// Gana el cambio más reciente (`actualizado`, ms del equipo que lo hizo). Si el
// servidor tiene algo más nuevo, el cambio se descarta y se devuelve lo vigente
// para que el equipo se ponga al día.
function subir(PDO $pdo, array $s, array $b): array
{
    $perfil = perfil_propio($pdo, $s['usuario'], $b['perfil'] ?? 0);
    $cambios = $b['cambios'] ?? null;
    if (!is_array($cambios) || count($cambios) > LOTE) {
        throw new Fallo(400, "'cambios' debe ser una lista de hasta " . LOTE);
    }
    $actual = $pdo->prepare('SELECT * FROM registros WHERE perfil_id = ? AND coleccion = ? AND clave = ?');
    $guardar = $pdo->prepare(
        'INSERT OR REPLACE INTO registros (perfil_id, coleccion, clave, datos, borrado, actualizado, seq)
         VALUES (?, ?, ?, ?, ?, ?, ?)'
    );
    $resultados = [];
    $pdo->exec('BEGIN IMMEDIATE');
    try {
        foreach ($cambios as $c) {
            if (!is_array($c)) {
                throw new Fallo(400, 'cambio inválido');
            }
            $coleccion = $c['coleccion'] ?? '';
            $clave = $c['clave'] ?? '';
            $borrado = !empty($c['borrado']);
            $act = $c['actualizado'] ?? null;
            if (!is_string($coleccion) || !preg_match('/^[a-z_]{1,32}$/', $coleccion)
                || !is_string($clave) || $clave === '' || strlen($clave) > 300
                || !is_int($act) || $act <= 0) {
                throw new Fallo(400, 'cambio inválido');
            }
            $datos = null;
            if (!$borrado) {
                if (!is_array($c['datos'] ?? null)) {
                    throw new Fallo(400, "cambio sin 'datos'");
                }
                $datos = json_encode($c['datos'], JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES);
                if (strlen($datos) > 64 * 1024) {
                    throw new Fallo(413, 'registro demasiado grande');
                }
            }
            $actual->execute([$perfil, $coleccion, $clave]);
            $previo = $actual->fetch();
            if ($previo && (int) $previo['actualizado'] > $act) {
                $resultados[] = ['ok' => false, 'vigente' => fila_registro($previo)];
                continue;
            }
            $seq = siguiente_seq($pdo);
            $guardar->execute([$perfil, $coleccion, $clave, $datos, $borrado ? 1 : 0, $act, $seq]);
            $resultados[] = ['ok' => true, 'seq' => $seq];
        }
        $pdo->exec('COMMIT');
    } catch (Throwable $e) {
        $pdo->exec('ROLLBACK');
        throw $e;
    }
    return ['resultados' => $resultados];
}

// ─── Amigos ─────────────────────────────────────────────────────────────────

function id_de_nombre(PDO $pdo, string $nombre): int
{
    $st = $pdo->prepare('SELECT id FROM usuarios WHERE nombre = ?');
    $st->execute([strtolower(trim($nombre))]);
    $id = $st->fetchColumn();
    if ($id === false) {
        throw new Fallo(404, 'no hay ningún usuario con ese nombre en este servidor');
    }
    return (int) $id;
}

function son_amigos(PDO $pdo, int $a, int $b): bool
{
    $st = $pdo->prepare(
        'SELECT 1 FROM amistades WHERE aceptada = 1 AND ((de = ? AND para = ?) OR (de = ? AND para = ?))'
    );
    $st->execute([$a, $b, $b, $a]);
    return (bool) $st->fetchColumn();
}

function amigos(PDO $pdo, array $s): array
{
    $st = $pdo->prepare(
        'SELECT a.de, a.aceptada, a.creada, u.nombre FROM amistades a
           JOIN usuarios u ON u.id = CASE WHEN a.de = ? THEN a.para ELSE a.de END
          WHERE a.de = ? OR a.para = ?
          ORDER BY u.nombre'
    );
    $st->execute([$s['usuario'], $s['usuario'], $s['usuario']]);
    $out = ['amigos' => [], 'recibidas' => [], 'enviadas' => []];
    foreach ($st->fetchAll() as $f) {
        $grupo = $f['aceptada'] ? 'amigos' : ((int) $f['de'] === $s['usuario'] ? 'enviadas' : 'recibidas');
        $out[$grupo][] = ['nombre' => $f['nombre'], 'desde' => (int) $f['creada']];
    }
    return $out;
}

// Pedir la amistad a alguien que ya te la pidió = aceptarla.
function agregar_amigo(PDO $pdo, array $s, array $b): array
{
    $otro = id_de_nombre($pdo, texto($b, 'nombre', 32));
    if ($otro === $s['usuario']) {
        throw new Fallo(400, 'no puedes agregarte a ti mismo');
    }
    $acepta = $pdo->prepare('UPDATE amistades SET aceptada = 1 WHERE de = ? AND para = ?');
    $acepta->execute([$otro, $s['usuario']]);
    if ($acepta->rowCount() === 0) {
        $pdo->prepare('INSERT OR IGNORE INTO amistades (de, para, creada) VALUES (?, ?, ?)')
            ->execute([$s['usuario'], $otro, ahora_ms()]);
    }
    return amigos($pdo, $s);
}

// Rechazar, cancelar lo enviado o dejar de ser amigos: todo borra el par.
function quitar_amigo(PDO $pdo, array $s, string $nombre): array
{
    $otro = id_de_nombre($pdo, $nombre);
    $pdo->prepare('DELETE FROM amistades WHERE (de = ? AND para = ?) OR (de = ? AND para = ?)')
        ->execute([$s['usuario'], $otro, $otro, $s['usuario']]);
    cortar_prestamos($pdo, $s['usuario'], $otro);
    return amigos($pdo, $s);
}

// ─── Recomendaciones ────────────────────────────────────────────────────────

const BANDEJA = 100;

function recomendaciones(PDO $pdo, array $s): array
{
    $st = $pdo->prepare(
        'SELECT r.id, u.nombre AS de, r.datos, r.nota, r.creada, r.vista FROM recomendaciones r
           JOIN usuarios u ON u.id = r.de
          WHERE r.para = ? ORDER BY r.id DESC LIMIT ' . BANDEJA
    );
    $st->execute([$s['usuario']]);
    return ['recomendaciones' => array_map(function ($r) {
        return [
            'id' => (int) $r['id'],
            'de' => $r['de'],
            'datos' => json_decode($r['datos'], true),
            'nota' => $r['nota'],
            'creada' => (int) $r['creada'],
            'vista' => (bool) $r['vista'],
        ];
    }, $st->fetchAll())];
}

function recomendar(PDO $pdo, array $s, array $b): array
{
    $para = $b['para'] ?? null;
    if (!is_array($para) || !$para || count($para) > 50) {
        throw new Fallo(400, "'para' debe ser una lista de amigos");
    }
    $datos = $b['datos'] ?? null;
    // Un título, o una lista compartida (que viaja como su código).
    $es_lista = is_array($datos) && ($datos['tipo'] ?? '') === 'lista';
    if ($es_lista ? !isset($datos['codigo'], $datos['nombre'])
                  : (!is_array($datos) || !isset($datos['tmdb_id'], $datos['media_type'], $datos['title']))) {
        throw new Fallo(400, "faltan los 'datos' del título");
    }
    $json = json_encode($datos, JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES);
    if (strlen($json) > 4096) {
        throw new Fallo(413, 'datos demasiado grandes');
    }
    $nota = $b['nota'] ?? null;
    if ($nota !== null && (!is_string($nota) || largo($nota) > 200)) {
        throw new Fallo(400, 'la nota va hasta 200 caracteres');
    }
    $nota = $nota === null || trim($nota) === '' ? null : trim($nota);

    $ids = [];
    foreach ($para as $nombre) {
        if (!is_string($nombre)) {
            throw new Fallo(400, "'para' debe ser una lista de amigos");
        }
        $id = id_de_nombre($pdo, $nombre);
        if (!son_amigos($pdo, $s['usuario'], $id)) {
            throw new Fallo(403, "$nombre no es tu amigo");
        }
        $ids[$id] = true;
    }
    $st = $pdo->prepare('INSERT INTO recomendaciones (de, para, datos, nota, creada) VALUES (?, ?, ?, ?, ?)');
    foreach (array_keys($ids) as $id) {
        $st->execute([$s['usuario'], $id, $json, $nota, ahora_ms()]);
        // La bandeja guarda las últimas BANDEJA: lo más viejo se cae.
        $pdo->prepare(
            'DELETE FROM recomendaciones WHERE para = ? AND id NOT IN
               (SELECT id FROM recomendaciones WHERE para = ? ORDER BY id DESC LIMIT ' . BANDEJA . ')'
        )->execute([$id, $id]);
    }
    return ['ok' => true, 'enviadas' => count($ids)];
}

function marcar_vistas(PDO $pdo, array $s): array
{
    $pdo->prepare('UPDATE recomendaciones SET vista = 1 WHERE para = ? AND vista = 0')->execute([$s['usuario']]);
    return ['ok' => true];
}

function quitar_recomendacion(PDO $pdo, array $s, int $id): array
{
    $pdo->prepare('DELETE FROM recomendaciones WHERE id = ? AND para = ?')->execute([$id, $s['usuario']]);
    return ['ok' => true];
}

// ─── Listas compartidas ─────────────────────────────────────────────────────

const LISTA_MAX_ITEMS = 500;

function fila_publica(array $f): array
{
    return [
        'codigo' => $f['codigo'],
        'lista_id' => $f['lista_id'],
        'nombre' => $f['nombre'],
        'actualizada' => (int) $f['actualizada'],
    ];
}

function mis_publicas(PDO $pdo, array $s): array
{
    $st = $pdo->prepare('SELECT codigo, lista_id, nombre, actualizada FROM listas_publicas WHERE dueno = ?');
    $st->execute([$s['usuario']]);
    return ['publicas' => array_map('fila_publica', $st->fetchAll())];
}

// Publica (o actualiza) una lista. El código se mantiene entre
// actualizaciones: el que ya se compartió sigue sirviendo.
function publicar_lista(PDO $pdo, array $s, array $b): array
{
    $lista_id = texto($b, 'lista_id', 64);
    $nombre = texto($b, 'nombre', 60);
    $items = $b['items'] ?? null;
    if (!is_array($items) || count($items) > LISTA_MAX_ITEMS) {
        throw new Fallo(400, "'items' debe ser una lista de hasta " . LISTA_MAX_ITEMS);
    }
    foreach ($items as $it) {
        if (!is_array($it) || !isset($it['clave'], $it['tmdb_id'], $it['media_type'], $it['title'])) {
            throw new Fallo(400, 'ítem inválido');
        }
    }
    $json = json_encode(array_values($items), JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES);
    if (strlen($json) > 256 * 1024) {
        throw new Fallo(413, 'lista demasiado grande');
    }
    $st = $pdo->prepare(
        'UPDATE listas_publicas SET nombre = ?, items = ?, actualizada = ? WHERE dueno = ? AND lista_id = ?'
    );
    $st->execute([$nombre, $json, ahora_ms(), $s['usuario'], $lista_id]);
    if ($st->rowCount() === 0) {
        $pdo->prepare(
            'INSERT INTO listas_publicas (codigo, dueno, lista_id, nombre, items, actualizada) VALUES (?, ?, ?, ?, ?, ?)'
        )->execute([nuevo_codigo(), $s['usuario'], $lista_id, $nombre, $json, ahora_ms()]);
    }
    $st = $pdo->prepare('SELECT * FROM listas_publicas WHERE dueno = ? AND lista_id = ?');
    $st->execute([$s['usuario'], $lista_id]);
    return fila_publica($st->fetch());
}

function despublicar_lista(PDO $pdo, array $s, string $codigo): array
{
    $pdo->prepare('DELETE FROM listas_publicas WHERE codigo = ? AND dueno = ?')->execute([$codigo, $s['usuario']]);
    return mis_publicas($pdo, $s);
}

// Cualquier usuario con el código. Los códigos fallidos cuentan para el freno
// de intentos, como el login: no se pueden probar al azar.
function lista_publica(PDO $pdo, string $codigo): array
{
    frenar_si_abusa($pdo);
    $st = $pdo->prepare(
        'SELECT l.*, u.nombre AS de FROM listas_publicas l JOIN usuarios u ON u.id = l.dueno WHERE l.codigo = ?'
    );
    $st->execute([strtoupper($codigo)]);
    $f = $st->fetch();
    if (!$f) {
        anotar_fallo($pdo);
        throw new Fallo(404, 'no hay ninguna lista con ese código (o su dueño dejó de compartirla)');
    }
    return fila_publica($f) + ['de' => $f['de'], 'items' => json_decode($f['items'], true)];
}

// ─── Llavero ────────────────────────────────────────────────────────────────

const SERVICIOS = ['rd'];
// Renovar con este margen antes de que venza, y nunca más de una vez por
// minuto aunque un equipo diga que RD le rechazó el token.
const LLAVE_MARGEN_S = 3600;
const LLAVE_PAUSA_MS = 60000;

function servicio_valido(string $servicio): string
{
    if (!in_array($servicio, SERVICIOS, true)) {
        throw new Fallo(404, "servicio desconocido: $servicio");
    }
    return $servicio;
}

function llave_propia(PDO $pdo, int $usuario, string $servicio): ?array
{
    $st = $pdo->prepare('SELECT * FROM llavero WHERE dueno = ? AND servicio = ?');
    $st->execute([$usuario, $servicio]);
    $f = $st->fetch();
    return $f ?: null;
}

function llavero(PDO $pdo, array $s): array
{
    $mias = [];
    $st = $pdo->prepare('SELECT id, servicio, creado FROM llavero WHERE dueno = ?');
    $st->execute([$s['usuario']]);
    $prest = $pdo->prepare(
        'SELECT u.nombre, p.creado, p.ultimo_uso FROM prestamos p JOIN usuarios u ON u.id = p.para
          WHERE p.llave = ? ORDER BY u.nombre'
    );
    foreach ($st->fetchAll() as $l) {
        $prest->execute([$l['id']]);
        $mias[$l['servicio']] = [
            'creada' => (int) $l['creado'],
            'prestada_a' => array_map(function ($p) {
                return [
                    'nombre' => $p['nombre'],
                    'desde' => (int) $p['creado'],
                    'ultimo_uso' => $p['ultimo_uso'] === null ? null : (int) $p['ultimo_uso'],
                ];
            }, $prest->fetchAll()),
        ];
    }
    $st = $pdo->prepare(
        'SELECT l.servicio, u.nombre AS de, p.creado FROM prestamos p
           JOIN llavero l ON l.id = p.llave JOIN usuarios u ON u.id = l.dueno
          WHERE p.para = ? ORDER BY p.creado'
    );
    $st->execute([$s['usuario']]);
    $prestadas = array_map(function ($p) {
        return ['servicio' => $p['servicio'], 'de' => $p['de'], 'desde' => (int) $p['creado']];
    }, $st->fetchAll());
    return ['mias' => (object) $mias, 'prestadas' => $prestadas];
}

// El dueño sube la vinculación hecha solo para el llavero. Volver a subirla
// (revincular) conserva los préstamos.
function guardar_llave(PDO $pdo, array $s, string $servicio, array $b): array
{
    $v = [
        texto($b, 'client_id', 200),
        texto($b, 'client_secret', 200),
        texto($b, 'refresh_token', 500),
        texto($b, 'access_token', 500),
    ];
    $expira = $b['expira'] ?? null;
    if (!is_int($expira) || $expira <= 0) {
        throw new Fallo(400, "falta 'expira'");
    }
    $st = $pdo->prepare(
        'UPDATE llavero SET client_id = ?, client_secret = ?, refresh_token = ?, access_token = ?,
                expira = ?, renovado = 0 WHERE dueno = ? AND servicio = ?'
    );
    $st->execute(array_merge($v, [$expira, $s['usuario'], $servicio]));
    if ($st->rowCount() === 0) {
        $pdo->prepare(
            'INSERT INTO llavero (dueno, servicio, client_id, client_secret, refresh_token, access_token, expira, creado)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)'
        )->execute(array_merge([$s['usuario'], $servicio], $v, [$expira, ahora_ms()]));
    }
    return llavero($pdo, $s);
}

function borrar_llave(PDO $pdo, array $s, string $servicio): array
{
    $llave = llave_propia($pdo, $s['usuario'], $servicio);
    if ($llave) {
        anular_token($pdo, $llave);
    }
    $pdo->prepare('DELETE FROM llavero WHERE dueno = ? AND servicio = ?')->execute([$s['usuario'], $servicio]);
    return llavero($pdo, $s);
}

function prestar(PDO $pdo, array $s, string $servicio, array $b): array
{
    $llave = llave_propia($pdo, $s['usuario'], $servicio);
    if (!$llave) {
        throw new Fallo(404, 'primero vincula la cuenta que vas a prestar');
    }
    $nombre = texto($b, 'nombre', 32);
    $otro = id_de_nombre($pdo, $nombre);
    if (!son_amigos($pdo, $s['usuario'], $otro)) {
        throw new Fallo(403, "$nombre no es tu amigo");
    }
    $pdo->prepare('INSERT OR IGNORE INTO prestamos (llave, para, creado) VALUES (?, ?, ?)')
        ->execute([$llave['id'], $otro, ahora_ms()]);
    return llavero($pdo, $s);
}

function dejar_de_prestar(PDO $pdo, array $s, string $servicio, string $nombre): array
{
    $llave = llave_propia($pdo, $s['usuario'], $servicio);
    if ($llave) {
        $st = $pdo->prepare('DELETE FROM prestamos WHERE llave = ? AND para = ?');
        $st->execute([$llave['id'], id_de_nombre($pdo, $nombre)]);
        if ($st->rowCount() > 0) {
            anular_token($pdo, $llave);
        }
    }
    return llavero($pdo, $s);
}

// Dejar de ser amigos corta los préstamos en los dos sentidos.
function cortar_prestamos(PDO $pdo, int $a, int $b): void
{
    foreach ([[$a, $b], [$b, $a]] as list($dueno, $para)) {
        $st = $pdo->prepare('SELECT * FROM llavero WHERE dueno = ?');
        $st->execute([$dueno]);
        foreach ($st->fetchAll() as $llave) {
            $del = $pdo->prepare('DELETE FROM prestamos WHERE llave = ? AND para = ?');
            $del->execute([$llave['id'], $para]);
            if ($del->rowCount() > 0) {
                anular_token($pdo, $llave);
            }
        }
    }
}

// Corta al instante el token que tenían los que recibían el préstamo (probado
// contra RD: anula solo ese token; los de los equipos del dueño siguen y la
// vinculación sirve para renovar). Los que siguen con préstamo reciben uno
// nuevo en su próximo pedido, porque se marca vencido.
function anular_token(PDO $pdo, array $llave): void
{
    list($estado) = http_pedir('GET', config()['rd_api'] . '/disable_access_token', null, $llave['access_token']);
    if ($estado !== 204 && $estado !== 200) {
        error_log("[kutral] llave {$llave['id']}: disable_access_token respondió $estado");
    }
    $pdo->prepare('UPDATE llavero SET expira = 0, renovado = 0 WHERE id = ?')->execute([$llave['id']]);
}

function http_post_form(string $url, array $form): array
{
    return http_pedir('POST', $url, $form, null);
}

function http_pedir(string $metodo, string $url, ?array $form, ?string $token): array
{
    $cuerpo = $form === null ? null : http_build_query($form);
    $headers = [];
    if ($cuerpo !== null) {
        $headers[] = 'Content-Type: application/x-www-form-urlencoded';
    }
    if ($token !== null) {
        $headers[] = 'Authorization: Bearer ' . $token;
    }
    if (function_exists('curl_init')) {
        $ch = curl_init($url);
        $op = [
            CURLOPT_CUSTOMREQUEST => $metodo,
            CURLOPT_HTTPHEADER => $headers,
            CURLOPT_RETURNTRANSFER => true,
            CURLOPT_TIMEOUT => 15,
        ];
        if ($cuerpo !== null) {
            $op[CURLOPT_POSTFIELDS] = $cuerpo;
        }
        curl_setopt_array($ch, $op);
        $resp = curl_exec($ch);
        $estado = (int) curl_getinfo($ch, CURLINFO_RESPONSE_CODE);
        curl_close($ch);
    } else {
        $http = [
            'method' => $metodo,
            'header' => implode("\r\n", $headers),
            'timeout' => 15,
            'ignore_errors' => true,
        ];
        if ($cuerpo !== null) {
            $http['content'] = $cuerpo;
        }
        $resp = @file_get_contents($url, false, stream_context_create(['http' => $http]));
        $estado = isset($http_response_header[0]) && preg_match('#\s(\d{3})\s#', $http_response_header[0], $m)
            ? (int) $m[1] : 0;
    }
    $json = is_string($resp) ? json_decode($resp, true) : null;
    return [$estado, is_array($json) ? $json : []];
}

function renovar_llave(PDO $pdo, array $llave, string $de): array
{
    list($estado, $t) = http_post_form(config()['rd_oauth'] . '/token', [
        'client_id' => $llave['client_id'],
        'client_secret' => $llave['client_secret'],
        'code' => $llave['refresh_token'],
        'grant_type' => 'http://oauth.net/grant_type/device/1.0',
    ]);
    if ($estado !== 200 || empty($t['access_token'])) {
        error_log("[kutral] llave {$llave['id']} no renovó: $estado");
        throw new Fallo(502, "la cuenta que te presta $de ya no responde; pídele que la vuelva a vincular");
    }
    $llave['access_token'] = $t['access_token'];
    $llave['refresh_token'] = $t['refresh_token'] ?? $llave['refresh_token'];
    $llave['expira'] = time() + (int) ($t['expires_in'] ?? 86400);
    $llave['renovado'] = ahora_ms();
    $pdo->prepare('UPDATE llavero SET access_token = ?, refresh_token = ?, expira = ?, renovado = ? WHERE id = ?')
        ->execute([$llave['access_token'], $llave['refresh_token'], $llave['expira'], $llave['renovado'], $llave['id']]);
    return $llave;
}

// Lo que pide el equipo de quien recibe el préstamo. `renovar=1` cuando RD le
// acaba de rechazar el token que tenía.
function token_prestado(PDO $pdo, array $s, string $servicio): array
{
    $st = $pdo->prepare(
        'SELECT l.*, u.nombre AS de FROM prestamos p
           JOIN llavero l ON l.id = p.llave JOIN usuarios u ON u.id = l.dueno
          WHERE p.para = ? AND l.servicio = ? ORDER BY p.creado LIMIT 1'
    );
    $st->execute([$s['usuario'], $servicio]);
    $llave = $st->fetch();
    if (!$llave) {
        throw new Fallo(403, 'nadie te está prestando esa cuenta');
    }
    $vence = (int) $llave['expira'] <= time() + LLAVE_MARGEN_S;
    $pide = ($_GET['renovar'] ?? '') === '1' && ahora_ms() - (int) $llave['renovado'] > LLAVE_PAUSA_MS;
    if ($vence || $pide) {
        $llave = renovar_llave($pdo, $llave, $llave['de']);
    }
    $pdo->prepare('UPDATE prestamos SET ultimo_uso = ? WHERE llave = ? AND para = ?')
        ->execute([ahora_ms(), $llave['id'], $s['usuario']]);
    return ['access_token' => $llave['access_token'], 'expira' => (int) $llave['expira'], 'de' => $llave['de']];
}

// ─── Despacho ───────────────────────────────────────────────────────────────

try {
    $metodo = $_SERVER['REQUEST_METHOD'] ?? 'GET';
    $r = ruta();
    if ($metodo === 'GET' && $r === '/ping') {
        responder(200, ping());
    }
    $pdo = db();
    if ($r === '/admin') {
        require __DIR__ . '/../panel.php';
        panel($pdo, $metodo);
    }
    if ($metodo === 'POST' && $r === '/registro') {
        $res = registro($pdo, cuerpo());
    } elseif ($metodo === 'POST' && $r === '/login') {
        $res = login($pdo, cuerpo());
    } elseif ($metodo === 'POST' && $r === '/salir') {
        $s = autenticar($pdo);
        $res = quitar_equipo($pdo, $s, $s['equipo']);
    } elseif ($metodo === 'GET' && $r === '/yo') {
        $res = yo($pdo, autenticar($pdo));
    } elseif ($metodo === 'POST' && $r === '/yo/clave') {
        $res = verificar_clave($pdo, autenticar($pdo), cuerpo());
    } elseif ($metodo === 'GET' && $r === '/boveda') {
        $res = boveda($pdo, autenticar($pdo));
    } elseif ($metodo === 'PUT' && $r === '/boveda') {
        $res = guardar_boveda($pdo, autenticar($pdo), cuerpo());
    } elseif ($metodo === 'DELETE' && preg_match('#^/equipos/(\d+)$#', $r, $m) === 1) {
        $res = quitar_equipo($pdo, autenticar($pdo), (int) $m[1]);
    } elseif ($metodo === 'GET' && $r === '/sync') {
        $res = bajar($pdo, autenticar($pdo));
    } elseif ($metodo === 'POST' && $r === '/sync') {
        $res = subir($pdo, autenticar($pdo), cuerpo());
    } elseif ($metodo === 'GET' && $r === '/perfiles') {
        $s = autenticar($pdo);
        $res = ['perfiles' => perfiles($pdo, $s['usuario'])];
    } elseif ($metodo === 'POST' && $r === '/perfiles') {
        $res = guardar_perfil($pdo, autenticar($pdo), cuerpo());
    } elseif ($metodo === 'DELETE' && preg_match('#^/perfiles/(\d+)$#', $r, $m) === 1) {
        $res = borrar_perfil($pdo, autenticar($pdo), (int) $m[1]);
    } elseif ($metodo === 'GET' && $r === '/amigos') {
        $res = amigos($pdo, autenticar($pdo));
    } elseif ($metodo === 'POST' && $r === '/amigos') {
        $res = agregar_amigo($pdo, autenticar($pdo), cuerpo());
    } elseif ($metodo === 'DELETE' && preg_match('#^/amigos/([a-z0-9_.-]{1,32})$#', $r, $m) === 1) {
        $res = quitar_amigo($pdo, autenticar($pdo), $m[1]);
    } elseif ($metodo === 'GET' && $r === '/recomendaciones') {
        $res = recomendaciones($pdo, autenticar($pdo));
    } elseif ($metodo === 'POST' && $r === '/recomendaciones') {
        $res = recomendar($pdo, autenticar($pdo), cuerpo());
    } elseif ($metodo === 'POST' && $r === '/recomendaciones/vistas') {
        $res = marcar_vistas($pdo, autenticar($pdo));
    } elseif ($metodo === 'DELETE' && preg_match('#^/recomendaciones/(\d+)$#', $r, $m) === 1) {
        $res = quitar_recomendacion($pdo, autenticar($pdo), (int) $m[1]);
    } elseif ($metodo === 'GET' && $r === '/listas-publicas') {
        $res = mis_publicas($pdo, autenticar($pdo));
    } elseif ($metodo === 'POST' && $r === '/listas-publicas') {
        $res = publicar_lista($pdo, autenticar($pdo), cuerpo());
    } elseif (preg_match('#^/listas-publicas/([A-Za-z0-9]{5}-[A-Za-z0-9]{5})$#', $r, $m) === 1
        && ($metodo === 'GET' || $metodo === 'DELETE')) {
        $s = autenticar($pdo);
        $res = $metodo === 'GET' ? lista_publica($pdo, $m[1]) : despublicar_lista($pdo, $s, strtoupper($m[1]));
    } elseif ($metodo === 'GET' && $r === '/llavero') {
        $res = llavero($pdo, autenticar($pdo));
    } elseif (preg_match('#^/llavero/([a-z]{1,16})(/.*)?$#', $r, $m) === 1) {
        $servicio = servicio_valido($m[1]);
        $resto = $m[2] ?? '';
        $s = autenticar($pdo);
        if ($metodo === 'POST' && $resto === '') {
            $res = guardar_llave($pdo, $s, $servicio, cuerpo());
        } elseif ($metodo === 'DELETE' && $resto === '') {
            $res = borrar_llave($pdo, $s, $servicio);
        } elseif ($metodo === 'POST' && $resto === '/prestamos') {
            $res = prestar($pdo, $s, $servicio, cuerpo());
        } elseif ($metodo === 'DELETE' && preg_match('#^/prestamos/([a-z0-9_.-]{1,32})$#', $resto, $n) === 1) {
            $res = dejar_de_prestar($pdo, $s, $servicio, $n[1]);
        } elseif ($metodo === 'GET' && $resto === '/token') {
            $res = token_prestado($pdo, $s, $servicio);
        } else {
            throw new Fallo(404, "no existe $metodo $r");
        }
    } else {
        throw new Fallo(404, "no existe $metodo $r");
    }
    responder(200, $res);
} catch (Fallo $e) {
    responder($e->estado, ['error' => $e->getMessage()]);
} catch (Throwable $e) {
    error_log('[kutral] ' . $e);
    responder(500, ['error' => 'error interno']);
}
