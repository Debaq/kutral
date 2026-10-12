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
    $st = $pdo->prepare('SELECT id, nombre FROM perfiles WHERE usuario_id = ? ORDER BY id');
    $st->execute([$usuario]);
    return array_map(fn($p) => ['id' => (int) $p['id'], 'nombre' => $p['nombre']], $st->fetchAll());
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
    if (!is_array($datos) || !isset($datos['tmdb_id'], $datos['media_type'], $datos['title'])) {
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
    } elseif ($metodo === 'DELETE' && preg_match('#^/equipos/(\d+)$#', $r, $m) === 1) {
        $res = quitar_equipo($pdo, autenticar($pdo), (int) $m[1]);
    } elseif ($metodo === 'GET' && $r === '/sync') {
        $res = bajar($pdo, autenticar($pdo));
    } elseif ($metodo === 'POST' && $r === '/sync') {
        $res = subir($pdo, autenticar($pdo), cuerpo());
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
