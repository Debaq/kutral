<?php
// Administración por consola. No es accesible por web (está fuera de public/).
//
//   php admin.php invitar [cantidad]   códigos de registro de un solo uso
//   php admin.php invitaciones         pendientes y usadas
//   php admin.php usuarios             usuarios con sus equipos
//   php admin.php clave <nombre>       cambia la contraseña (la pide por consola)
//   php admin.php borrar <nombre>      borra el usuario y todos sus datos
//   php admin.php clave-admin          cambia la contraseña del panel web
//
// Lo mismo se hace desde el panel web en /admin; esto sirve para recuperar
// el acceso al panel si se olvida su contraseña.

declare(strict_types=1);

if (PHP_SAPI !== 'cli') {
    http_response_code(404);
    exit;
}

require __DIR__ . '/lib.php';

function pedir_clave(): string
{
    fwrite(STDOUT, 'Nueva contraseña: ');
    system('stty -echo 2>/dev/null');
    $c = trim((string) fgets(STDIN));
    system('stty echo 2>/dev/null');
    fwrite(STDOUT, "\n");
    return $c;
}

$pdo = db();
$cmd = $argv[1] ?? '';

switch ($cmd) {
    case 'invitar':
        foreach (crear_invitaciones($pdo, max(1, (int) ($argv[2] ?? 1))) as $codigo) {
            echo $codigo, "\n";
        }
        break;

    case 'invitaciones':
        $filas = $pdo->query(
            'SELECT i.codigo, i.creada, i.usada_en, u.nombre FROM invitaciones i
               LEFT JOIN usuarios u ON u.id = i.usada_por ORDER BY i.creada'
        )->fetchAll();
        foreach ($filas as $f) {
            echo $f['codigo'], '  ', $f['usada_en'] ? 'usada por ' . ($f['nombre'] ?? '?') . ' el ' . fecha((int) $f['usada_en']) : 'libre', "\n";
        }
        break;

    case 'usuarios':
        foreach ($pdo->query('SELECT id, nombre, creado FROM usuarios ORDER BY nombre')->fetchAll() as $u) {
            echo $u['nombre'], '  (desde ', fecha((int) $u['creado']), ")\n";
            $st = $pdo->prepare('SELECT nombre, ultimo_uso FROM equipos WHERE usuario_id = ? ORDER BY ultimo_uso DESC');
            $st->execute([$u['id']]);
            foreach ($st->fetchAll() as $e) {
                echo '    ', $e['nombre'], '  último uso ', fecha((int) $e['ultimo_uso']), "\n";
            }
        }
        break;

    case 'clave':
        $nombre = strtolower($argv[2] ?? '');
        $clave = pedir_clave();
        if (largo($clave) < 8) {
            fwrite(STDERR, "Mínimo 8 caracteres.\n");
            exit(1);
        }
        if (!cambiar_clave($pdo, $nombre, $clave)) {
            fwrite(STDERR, "No existe '$nombre'.\n");
            exit(1);
        }
        echo "Listo. Se cerró la sesión en todos sus equipos.\n";
        break;

    case 'borrar':
        $nombre = strtolower($argv[2] ?? '');
        echo borrar_usuario($pdo, $nombre) ? "Borrado '$nombre'.\n" : "No existe '$nombre'.\n";
        break;

    case 'clave-admin':
        $clave = pedir_clave();
        if (largo($clave) < 8) {
            fwrite(STDERR, "Mínimo 8 caracteres.\n");
            exit(1);
        }
        guardar_clave_admin($pdo, $clave);
        echo "Listo. Se cerraron las sesiones abiertas del panel.\n";
        break;

    default:
        fwrite(STDERR, "Uso: php admin.php invitar [n] | invitaciones | usuarios | clave <nombre> | borrar <nombre> | clave-admin\n");
        exit(1);
}
