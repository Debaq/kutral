<?php
// Administración por consola. No es accesible por web (está fuera de public/).
//
//   php admin.php invitar [cantidad]   códigos de registro de un solo uso
//   php admin.php invitaciones         pendientes y usadas
//   php admin.php usuarios             usuarios con sus equipos
//   php admin.php clave <nombre>       cambia la contraseña (la pide por consola)
//   php admin.php borrar <nombre>      borra el usuario y todos sus datos

declare(strict_types=1);

if (PHP_SAPI !== 'cli') {
    http_response_code(404);
    exit;
}

require __DIR__ . '/lib.php';

function fecha(?int $ms): string
{
    return $ms ? date('Y-m-d H:i', intdiv($ms, 1000)) : '-';
}

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
        $n = max(1, (int) ($argv[2] ?? 1));
        $st = $pdo->prepare('INSERT INTO invitaciones (codigo, creada) VALUES (?, ?)');
        for ($i = 0; $i < $n; $i++) {
            $codigo = nuevo_codigo();
            $st->execute([$codigo, ahora_ms()]);
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
        $st = $pdo->prepare('UPDATE usuarios SET pass_hash = ? WHERE nombre = ?');
        $st->execute([password_hash($clave, PASSWORD_DEFAULT), $nombre]);
        if ($st->rowCount() === 0) {
            fwrite(STDERR, "No existe '$nombre'.\n");
            exit(1);
        }
        // Cambiar la contraseña cierra la sesión en todos sus equipos.
        $pdo->prepare('DELETE FROM equipos WHERE usuario_id = (SELECT id FROM usuarios WHERE nombre = ?)')
            ->execute([$nombre]);
        echo "Listo. Se cerró la sesión en todos sus equipos.\n";
        break;

    case 'borrar':
        $nombre = strtolower($argv[2] ?? '');
        $st = $pdo->prepare('DELETE FROM usuarios WHERE nombre = ?');
        $st->execute([$nombre]);
        echo $st->rowCount() ? "Borrado '$nombre'.\n" : "No existe '$nombre'.\n";
        break;

    default:
        fwrite(STDERR, "Uso: php admin.php invitar [n] | invitaciones | usuarios | clave <nombre> | borrar <nombre>\n");
        exit(1);
}
