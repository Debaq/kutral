<?php
// Copia este archivo como config.php y ajusta lo que necesites.
// Todo es opcional: sin config.php se usan estos mismos valores.
return [
    // Ruta del SQLite. Debe quedar FUERA de la carpeta pública del servidor web
    // y el usuario de PHP necesita poder escribir en su carpeta (WAL crea
    // archivos -wal y -shm al lado).
    'db' => __DIR__ . '/datos/kutral.sqlite',
    // Fallos de login/registro por IP antes de responder 429, y ventana en segundos.
    'intentos_max' => 10,
    'intentos_ventana' => 900,
];
