<?php
// Panel web del administrador, en /admin. Es lo único con HTML: el resto del
// servidor es la API JSON. La primera visita es la instalación (solo pide la
// contraseña del administrador); después, invitaciones y usuarios.
//
// Lo carga public/index.php, así que usa Fallo, frenar_si_abusa() y
// anotar_fallo() de ahí. Sin JavaScript: formularios y redirección tras POST.

declare(strict_types=1);

// Inactividad tras la que hay que volver a entrar.
const PANEL_INACTIVIDAD_MS = 2 * 3600 * 1000;
const PANEL_COOKIE = 'kutral_admin';

const PANEL_AVISOS = [
    'instalado' => 'Listo, el servidor ya tiene administrador. Crea una invitación para el primer usuario.',
    'invitadas' => 'Invitaciones creadas.',
    'anulada' => 'Invitación anulada.',
    'creado' => 'Usuario creado.',
    'clave' => 'Contraseña cambiada. Se cerró su sesión en todos sus equipos.',
    'sesiones' => 'Se cerró su sesión en todos sus equipos.',
    'borrado' => 'Usuario borrado con todos sus datos.',
    'mi_clave' => 'Tu contraseña cambió. Las demás sesiones del panel se cerraron.',
];

function h($s): string
{
    return htmlspecialchars((string) $s, ENT_QUOTES, 'UTF-8');
}

// Ruta de esta página tal como la pidió el navegador: sirve con y sin rewrite
// (/kutral/admin o /kutral/index.php/admin).
function panel_url(): string
{
    $path = rtrim(parse_url($_SERVER['REQUEST_URI'] ?? '/', PHP_URL_PATH) ?: '/', '/');
    return str_ends_with($path, '/admin') ? $path : '/admin';
}

function es_https(): bool
{
    return (!empty($_SERVER['HTTPS']) && $_SERVER['HTTPS'] !== 'off')
        || ($_SERVER['HTTP_X_FORWARDED_PROTO'] ?? '') === 'https';
}

// La URL que hay que poner en Kutral: la de esta página sin el /admin.
function url_servidor(): string
{
    $host = $_SERVER['HTTP_HOST'] ?? 'tu-servidor';
    return (es_https() ? 'https://' : 'http://') . $host . substr(panel_url(), 0, -strlen('/admin'));
}

function panel_campo(string $nombre, int $max = 200): string
{
    $v = $_POST[$nombre] ?? null;
    if (!is_string($v) || $v === '' || largo($v) > $max) {
        throw new Fallo(400, 'Completa todos los campos.');
    }
    return $v;
}

// Si el formulario trae 'repetir', tiene que coincidir.
function panel_clave_nueva(): string
{
    $clave = panel_campo('clave');
    if (largo($clave) < 8) {
        throw new Fallo(400, 'La contraseña necesita al menos 8 caracteres.');
    }
    if (isset($_POST['repetir']) && $_POST['repetir'] !== $clave) {
        throw new Fallo(400, 'Las contraseñas no coinciden.');
    }
    return $clave;
}

function panel_cookie(string $valor): void
{
    setcookie(PANEL_COOKIE, $valor, [
        'expires' => $valor === '' ? 1 : 0,
        'path' => panel_url(),
        'secure' => es_https(),
        'httponly' => true,
        'samesite' => 'Strict',
    ]);
}

// Devuelve el token si hay sesión vigente (y la renueva).
function panel_sesion(PDO $pdo): ?string
{
    $token = $_COOKIE[PANEL_COOKIE] ?? '';
    if (!is_string($token) || !preg_match('/^[0-9a-f]{64}$/', $token)) {
        return null;
    }
    $ahora = ahora_ms();
    $st = $pdo->prepare('UPDATE admin_sesiones SET ultimo_uso = ? WHERE token_hash = ? AND ultimo_uso >= ?');
    $st->execute([$ahora, hash('sha256', $token), $ahora - PANEL_INACTIVIDAD_MS]);
    return $st->rowCount() ? $token : null;
}

function panel_abrir_sesion(PDO $pdo): void
{
    $token = bin2hex(random_bytes(32));
    $ahora = ahora_ms();
    $pdo->prepare('DELETE FROM admin_sesiones WHERE ultimo_uso < ?')->execute([$ahora - PANEL_INACTIVIDAD_MS]);
    $pdo->prepare('INSERT INTO admin_sesiones (token_hash, ultimo_uso) VALUES (?, ?)')
        ->execute([hash('sha256', $token), $ahora]);
    panel_cookie($token);
}

// Contra CSRF, además de SameSite=Strict: cada formulario lleva esto.
function panel_csrf(string $token): string
{
    return hash_hmac('sha256', 'csrf', $token);
}

// ─── Acciones (POST) ────────────────────────────────────────────────────────

// Devuelve la clave del aviso a mostrar tras redirigir ('' = ninguno).
function panel_post(PDO $pdo, string $accion): string
{
    if (clave_admin($pdo) === null) {
        if ($accion !== 'instalar') {
            return '';
        }
        $clave = panel_clave_nueva();
        $pdo->exec('BEGIN IMMEDIATE');
        try {
            if (clave_admin($pdo) !== null) {
                throw new Fallo(409, 'El servidor ya tiene administrador.');
            }
            guardar_clave_admin($pdo, $clave);
            $pdo->exec('COMMIT');
        } catch (Throwable $e) {
            $pdo->exec('ROLLBACK');
            throw $e;
        }
        panel_abrir_sesion($pdo);
        return 'instalado';
    }

    if ($accion === 'entrar') {
        frenar_si_abusa($pdo);
        if (!password_verify(panel_campo('clave'), (string) clave_admin($pdo))) {
            anotar_fallo($pdo);
            throw new Fallo(401, 'Contraseña incorrecta.');
        }
        panel_abrir_sesion($pdo);
        return '';
    }

    $token = panel_sesion($pdo);
    if ($token === null) {
        return ''; // sesión vencida: vuelve a pedir la contraseña
    }
    $csrf = $_POST['csrf'] ?? '';
    if (!is_string($csrf) || !hash_equals(panel_csrf($token), $csrf)) {
        throw new Fallo(403, 'El formulario venció. Vuelve a intentarlo.');
    }

    switch ($accion) {
        case 'salir':
            $pdo->prepare('DELETE FROM admin_sesiones WHERE token_hash = ?')->execute([hash('sha256', $token)]);
            panel_cookie('');
            return '';

        case 'invitar':
            crear_invitaciones($pdo, min(20, max(1, (int) ($_POST['cantidad'] ?? 1))));
            return 'invitadas';

        case 'anular':
            $pdo->prepare('DELETE FROM invitaciones WHERE codigo = ? AND usada_en IS NULL')
                ->execute([panel_campo('codigo', 20)]);
            return 'anulada';

        case 'crear':
            $nombre = strtolower(trim(panel_campo('nombre', 32)));
            if (!nombre_valido($nombre)) {
                throw new Fallo(400, 'El nombre va de 3 a 32 caracteres: letras, números, punto, guion o guion bajo.');
            }
            $clave = panel_clave_nueva();
            $st = $pdo->prepare('SELECT 1 FROM usuarios WHERE nombre = ?');
            $st->execute([$nombre]);
            if ($st->fetchColumn()) {
                throw new Fallo(409, 'Ese nombre ya existe.');
            }
            crear_usuario($pdo, $nombre, $clave);
            return 'creado';

        case 'clave':
            if (!cambiar_clave($pdo, panel_campo('nombre', 32), panel_clave_nueva())) {
                throw new Fallo(404, 'Ese usuario ya no existe.');
            }
            return 'clave';

        case 'sesiones':
            cerrar_sesiones($pdo, panel_campo('nombre', 32));
            return 'sesiones';

        case 'borrar':
            borrar_usuario($pdo, panel_campo('nombre', 32));
            return 'borrado';

        case 'mi_clave':
            if (!password_verify(panel_campo('actual'), (string) clave_admin($pdo))) {
                throw new Fallo(403, 'La contraseña actual no es esa.');
            }
            guardar_clave_admin($pdo, panel_clave_nueva());
            panel_abrir_sesion($pdo);
            return 'mi_clave';
    }
    throw new Fallo(400, 'Acción desconocida.');
}

// ─── Páginas ────────────────────────────────────────────────────────────────

function panel(PDO $pdo, string $metodo): void
{
    $error = null;
    if ($metodo === 'POST') {
        try {
            $aviso = panel_post($pdo, (string) ($_POST['accion'] ?? ''));
            header('Location: ' . panel_url() . ($aviso !== '' ? '?m=' . $aviso : ''), true, 303);
            exit;
        } catch (Fallo $e) {
            $error = $e;
        }
    } elseif ($metodo !== 'GET') {
        throw new Fallo(405, "no existe $metodo /admin");
    }

    $estado = $error ? $error->estado : 200;
    $mensaje = $error ? ucfirst($error->getMessage()) : null;
    if (clave_admin($pdo) === null) {
        panel_html($estado, 'Instalar', vista_instalar($mensaje));
    }
    $token = panel_sesion($pdo);
    if ($token === null) {
        panel_html($estado, 'Entrar', vista_entrar($mensaje));
    }
    $m = $_GET['m'] ?? '';
    $aviso = is_string($m) ? (PANEL_AVISOS[$m] ?? null) : null;
    panel_html($estado, 'Administrar', vista_panel($pdo, $token, $mensaje, $aviso));
}

function panel_html(int $estado, string $titulo, string $cuerpo): void
{
    http_response_code($estado);
    header('Content-Type: text/html; charset=utf-8');
    header('Cache-Control: no-store');
    header("Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; frame-ancestors 'none'");
    header('X-Frame-Options: DENY');
    header('Referrer-Policy: no-referrer');
    ?>
<!doctype html>
<html lang="es">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex">
<title><?= h($titulo) ?> · Kutral</title>
<style>
  :root { color-scheme: dark; --fondo: #121214; --caja: #1c1c20; --borde: #2e2e35; --tenue: #9a9aa5; --acento: #ff7a33; --mal: #ff5c5c; }
  * { box-sizing: border-box; }
  body { margin: 0; background: var(--fondo); color: #eee; font: 15px/1.5 system-ui, sans-serif; }
  main { max-width: 860px; margin: 0 auto; padding: 24px 16px 64px; }
  header { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 20px; }
  h1 { font-size: 22px; margin: 0; } h1 span { color: var(--acento); }
  h2 { font-size: 17px; margin: 0 0 12px; }
  section { background: var(--caja); border: 1px solid var(--borde); border-radius: 10px; padding: 18px; margin-bottom: 18px; }
  .angosto { max-width: 380px; margin: 12vh auto 0; }
  label { display: block; margin-bottom: 10px; color: var(--tenue); font-size: 13px; }
  input { display: block; width: 100%; margin-top: 4px; padding: 8px 10px; background: var(--fondo); color: #eee; border: 1px solid var(--borde); border-radius: 6px; font: inherit; }
  input:focus { outline: 2px solid var(--acento); border-color: transparent; }
  button { padding: 8px 14px; background: var(--acento); color: #111; border: 0; border-radius: 6px; font: inherit; font-weight: 600; cursor: pointer; }
  button.sec { background: transparent; color: #eee; border: 1px solid var(--borde); font-weight: 400; }
  button.peligro { background: var(--mal); }
  form.fila { display: flex; gap: 8px; align-items: end; flex-wrap: wrap; }
  form.fila label { margin: 0; flex: 1; min-width: 140px; }
  form.fila label.corto { flex: 0 0 100px; min-width: 0; }
  table { width: 100%; border-collapse: collapse; }
  th, td { text-align: left; padding: 8px 6px; border-top: 1px solid var(--borde); vertical-align: top; }
  th { color: var(--tenue); font-weight: 400; font-size: 13px; border-top: 0; }
  code { font: 600 16px/1 ui-monospace, monospace; letter-spacing: .05em; }
  details summary { cursor: pointer; color: var(--acento); }
  details[open] summary { margin-bottom: 10px; }
  details form { margin-bottom: 10px; }
  .tenue { color: var(--tenue); font-size: 13px; }
  .aviso, .error { padding: 10px 14px; border-radius: 8px; margin-bottom: 18px; }
  .aviso { background: #1f3326; border: 1px solid #2f6b42; }
  .error { background: #3a1f22; border: 1px solid #7a2f36; }
  .vacio { color: var(--tenue); margin: 0; }
</style>
</head>
<body><main><?= $cuerpo ?></main></body>
</html>
<?php
    exit;
}

function vista_avisos(?string $error, ?string $aviso = null): string
{
    return ($aviso ? '<div class="aviso">' . h($aviso) . '</div>' : '')
        . ($error ? '<div class="error">' . h($error) . '</div>' : '');
}

function vista_instalar(?string $error): string
{
    ob_start(); ?>
<section class="angosto">
  <h1><span>Kutral</span> · instalación</h1>
  <p class="tenue">Elige la contraseña del administrador. Con ella entras a este
  panel para crear invitaciones y manejar usuarios.</p>
  <?= vista_avisos($error) ?>
  <form method="post" action="<?= h(panel_url()) ?>">
    <input type="hidden" name="accion" value="instalar">
    <label>Contraseña (mínimo 8 caracteres)
      <input type="password" name="clave" minlength="8" required autofocus autocomplete="new-password"></label>
    <label>Repítela
      <input type="password" name="repetir" minlength="8" required autocomplete="new-password"></label>
    <button>Instalar</button>
  </form>
</section>
<?php
    return (string) ob_get_clean();
}

function vista_entrar(?string $error): string
{
    ob_start(); ?>
<section class="angosto">
  <h1><span>Kutral</span> · administrador</h1>
  <?= vista_avisos($error) ?>
  <form method="post" action="<?= h(panel_url()) ?>">
    <input type="hidden" name="accion" value="entrar">
    <label>Contraseña
      <input type="password" name="clave" required autofocus autocomplete="current-password"></label>
    <button>Entrar</button>
  </form>
</section>
<?php
    return (string) ob_get_clean();
}

// Abre un formulario del panel con la acción y el token anti-CSRF.
function abrir_form(string $accion, string $token, string $clase = ''): string
{
    return '<form method="post" action="' . h(panel_url()) . '"' . ($clase ? ' class="' . $clase . '"' : '') . '>'
        . '<input type="hidden" name="accion" value="' . h($accion) . '">'
        . '<input type="hidden" name="csrf" value="' . h(panel_csrf($token)) . '">';
}

function vista_panel(PDO $pdo, string $token, ?string $error, ?string $aviso): string
{
    $libres = $pdo->query('SELECT codigo, creada FROM invitaciones WHERE usada_en IS NULL ORDER BY creada DESC')->fetchAll();
    $usadas = $pdo->query(
        'SELECT i.codigo, i.usada_en, u.nombre FROM invitaciones i
           LEFT JOIN usuarios u ON u.id = i.usada_por
          WHERE i.usada_en IS NOT NULL ORDER BY i.usada_en DESC LIMIT 20'
    )->fetchAll();
    $usuarios = $pdo->query(
        'SELECT u.nombre, u.creado, COUNT(e.id) AS equipos, MAX(e.ultimo_uso) AS ultimo
           FROM usuarios u LEFT JOIN equipos e ON e.usuario_id = u.id
          GROUP BY u.id ORDER BY u.nombre'
    )->fetchAll();
    ob_start(); ?>
<header>
  <h1><span>Kutral</span> · administrador</h1>
  <?= abrir_form('salir', $token) ?><button class="sec">Salir</button></form>
</header>
<?= vista_avisos($error, $aviso) ?>

<section>
  <h2>Invitaciones</h2>
  <p class="tenue">Cada código sirve para crear una sola cuenta. En Kutral:
  Configuración → Cuenta de Kutral → URL <code><?= h(url_servidor()) ?></code> →
  "Tengo una invitación: crear cuenta".</p>
  <?= abrir_form('invitar', $token, 'fila') ?>
    <label class="corto">Cantidad <input type="number" name="cantidad" value="1" min="1" max="20"></label>
    <button>Crear invitación</button>
  </form>
  <?php if ($libres): ?>
  <table>
    <tr><th>Código libre</th><th>Creado</th><th></th></tr>
    <?php foreach ($libres as $i): ?>
    <tr>
      <td><code><?= h($i['codigo']) ?></code></td>
      <td class="tenue"><?= h(fecha((int) $i['creada'])) ?></td>
      <td><?= abrir_form('anular', $token) ?>
        <input type="hidden" name="codigo" value="<?= h($i['codigo']) ?>">
        <button class="sec">Anular</button></form></td>
    </tr>
    <?php endforeach ?>
  </table>
  <?php else: ?>
  <p class="vacio">No hay invitaciones libres.</p>
  <?php endif ?>
  <?php if ($usadas): ?>
  <details>
    <summary>Usadas (últimas <?= count($usadas) ?>)</summary>
    <table>
      <?php foreach ($usadas as $i): ?>
      <tr>
        <td><code><?= h($i['codigo']) ?></code></td>
        <td><?= h($i['nombre'] ?? '(usuario borrado)') ?></td>
        <td class="tenue"><?= h(fecha((int) $i['usada_en'])) ?></td>
      </tr>
      <?php endforeach ?>
    </table>
  </details>
  <?php endif ?>
</section>

<section>
  <h2>Usuarios</h2>
  <?php if ($usuarios): ?>
  <table>
    <tr><th>Nombre</th><th>Desde</th><th>Equipos</th><th>Último uso</th><th></th></tr>
    <?php foreach ($usuarios as $u): $n = h($u['nombre']); ?>
    <tr>
      <td><?= $n ?></td>
      <td class="tenue"><?= h(fecha((int) $u['creado'])) ?></td>
      <td><?= (int) $u['equipos'] ?></td>
      <td class="tenue"><?= h(fecha($u['ultimo'] === null ? null : (int) $u['ultimo'])) ?></td>
      <td>
        <details>
          <summary>Administrar</summary>
          <?= abrir_form('clave', $token, 'fila') ?>
            <input type="hidden" name="nombre" value="<?= $n ?>">
            <label>Nueva contraseña <input type="password" name="clave" minlength="8" required autocomplete="new-password"></label>
            <button class="sec">Cambiar</button>
          </form>
          <?php if ((int) $u['equipos'] > 0): ?>
          <?= abrir_form('sesiones', $token) ?>
            <input type="hidden" name="nombre" value="<?= $n ?>">
            <button class="sec">Cerrar sesión en <?= (int) $u['equipos'] === 1 ? 'su equipo' : 'sus ' . (int) $u['equipos'] . ' equipos' ?></button>
          </form>
          <?php endif ?>
          <details>
            <summary>Borrar usuario</summary>
            <p class="tenue">Se borran su cuenta, perfiles y todo lo sincronizado. No se puede deshacer.</p>
            <?= abrir_form('borrar', $token) ?>
              <input type="hidden" name="nombre" value="<?= $n ?>">
              <button class="peligro">Sí, borrar a <?= $n ?></button>
            </form>
          </details>
        </details>
      </td>
    </tr>
    <?php endforeach ?>
  </table>
  <?php else: ?>
  <p class="vacio">Todavía no hay usuarios.</p>
  <?php endif ?>
  <details>
    <summary>Crear usuario sin invitación</summary>
    <?= abrir_form('crear', $token, 'fila') ?>
      <label>Nombre <input name="nombre" required minlength="3" maxlength="32" pattern="[A-Za-z0-9_.\-]+" autocomplete="off"></label>
      <label>Contraseña <input type="password" name="clave" minlength="8" required autocomplete="new-password"></label>
      <button>Crear</button>
    </form>
  </details>
</section>

<section>
  <details>
    <summary>Cambiar mi contraseña</summary>
    <?= abrir_form('mi_clave', $token) ?>
      <label>Actual <input type="password" name="actual" required autocomplete="current-password"></label>
      <label>Nueva <input type="password" name="clave" minlength="8" required autocomplete="new-password"></label>
      <label>Repítela <input type="password" name="repetir" minlength="8" required autocomplete="new-password"></label>
      <button>Cambiar</button>
    </form>
  </details>
</section>
<?php
    return (string) ob_get_clean();
}
