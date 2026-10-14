# Servidor de cuentas de Kutral

API JSON para sincronizar Kutral entre equipos, más un panel web del
administrador en `/admin`. Es opcional:
Kutral funciona sin cuenta. Hoy sincroniza los favoritos y permite agregar
amigos del mismo servidor, recomendarse títulos y prestarles tu Real-Debrid
(llavero); historial, listas y perfiles vienen después.

## Requisitos

- PHP 7.4 o superior (también 8.x) con `pdo_sqlite` (en Debian/Ubuntu: `php-sqlite3`; en Arch: `php-sqlite`).
- HTTPS. Kutral solo acepta `http://` para IPs de la red local.

## Instalar

1. Copia la carpeta `server/` al servidor, por ejemplo en `/srv/kutral`.
2. Solo `public/` debe quedar expuesta a la web. `lib.php`, `panel.php`,
   `admin.php`, `config.php` y `datos/` quedan fuera.
3. Opcional: `cp config.example.php config.php` y ajusta.
4. El usuario de PHP (`www-data`, `http`…) necesita escribir en `datos/`:
   `mkdir datos && chown www-data: datos && chmod 700 datos`.
5. Abre `https://tu-servidor/kutral/admin` y elige la contraseña del
   administrador. **Hazlo apenas lo publiques**: mientras no tenga contraseña,
   el primero que abra esa página se queda con el panel.
6. En el panel, crea una invitación.
7. En Kutral: Configuración → Cuenta de Kutral → URL del servidor → **Probar**,
   y después "Tengo una invitación: crear cuenta".

### nginx

No hay archivos estáticos: todo lo que cae bajo `/kutral/` va a `index.php`.

```nginx
location /kutral/ {
    include fastcgi_params;
    fastcgi_param SCRIPT_FILENAME /srv/kutral/public/index.php;
    fastcgi_param SCRIPT_NAME /kutral/index.php;
    fastcgi_pass unix:/run/php/php-fpm.sock;   # el socket de tu php-fpm
}
```

URL en Kutral: `https://tu-servidor/kutral`.

### Apache

`public/.htaccess` ya redirige todo a `index.php` y conserva el header
`Authorization`. Necesita `mod_rewrite` y `AllowOverride All` en esa carpeta.

### Hosting compartido (cPanel, LiteSpeed)

Si no puedes dejar nada fuera de la carpeta web, sube `server/` entera, por
ejemplo como `public_html/kutral/`. El `.htaccess` de la raíz manda todo a
`public/index.php`, así `lib.php`, `config.php` y `datos/` no se sirven.

URL en Kutral: `https://tu-servidor/kutral`.

### Sin reescritura de URLs

Funciona igual poniendo en Kutral la URL hasta `index.php`:
`https://tu-servidor/kutral/index.php`.

## Administrar

Desde el panel web (`/admin`): crear y anular invitaciones, crear usuarios sin
invitación, cambiarles la contraseña, cerrar su sesión en todos sus equipos y
borrarlos. La sesión del panel vence tras 2 horas sin uso.

Lo mismo por consola, que además sirve para recuperar el panel si se olvida
su contraseña:

```
php admin.php invitar [cantidad]   códigos de registro de un solo uso
php admin.php invitaciones         pendientes y usadas
php admin.php usuarios             usuarios con sus equipos
php admin.php clave <nombre>       cambia la contraseña y cierra todas sus sesiones
php admin.php borrar <nombre>      borra el usuario y todos sus datos
php admin.php clave-admin          cambia la contraseña del panel web
```

Respaldo: copiar `datos/kutral.sqlite` (con el servidor quieto, o con
`sqlite3 datos/kutral.sqlite ".backup respaldo.sqlite"`).

## API

Todo es JSON. Las rutas con 🔒 llevan `Authorization: Bearer <token>`.
Los errores responden `{"error": "mensaje"}` con su código HTTP.

| Ruta | Qué hace |
|---|---|
| `GET /ping` | `{"servicio":"kutral-cuentas","version":5}` |
| `POST /registro` | `{invitacion, nombre, clave, equipo}` → `{token, usuario, perfiles}` |
| `POST /login` | `{nombre, clave, equipo}` → `{token, usuario, perfiles}` |
| 🔒 `POST /salir` | Cierra la sesión de este equipo |
| 🔒 `GET /yo` | Usuario, perfiles y equipos con sesión |
| 🔒 `DELETE /equipos/{id}` | Cierra la sesión de otro equipo |
| 🔒 `POST /yo/clave` | `{clave}`: confirma la contraseña (403 si no es); cuenta para el freno de intentos |
| 🔒 `GET /boveda` | `{version, datos, actualizada}`; `version` 0 y `datos` null si no hay |
| 🔒 `PUT /boveda` | `{base, datos}` → `{version}`; 409 si `base` no es la versión actual |
| 🔒 `GET /perfiles` | `{perfiles: [{id, uid, nombre, color, actualizado}]}` |
| 🔒 `POST /perfiles` | `{uid, nombre, color, actualizado}`: crea, o actualiza si es más nuevo (hasta 10 por cuenta) |
| 🔒 `DELETE /perfiles/{id}` | Borra el perfil y todo lo sincronizado de él (el `principal` no) |
| 🔒 `GET /sync?perfil=ID&desde=SEQ` | Cambios posteriores a `SEQ`: `{cambios, hasta, mas}` |
| 🔒 `POST /sync` | `{perfil, cambios: [{coleccion, clave, datos, borrado, actualizado}]}` |
| 🔒 `GET /amigos` | `{amigos, recibidas, enviadas}`, cada uno `[{nombre, desde}]` |
| 🔒 `POST /amigos` | `{nombre}`: pide la amistad, o la acepta si el otro ya la pidió |
| 🔒 `DELETE /amigos/{nombre}` | Rechaza, cancela o deja de ser amigos |
| 🔒 `GET /recomendaciones` | Bandeja (últimas 100): `[{id, de, datos, nota, creada, vista}]` |
| 🔒 `POST /recomendaciones` | `{para: [nombres], datos: {tmdb_id, media_type, title, ...}, nota}`; solo a amigos. Una lista viaja como `datos: {tipo: "lista", codigo, nombre, cantidad}` |
| 🔒 `POST /recomendaciones/vistas` | Marca toda la bandeja como vista |
| 🔒 `DELETE /recomendaciones/{id}` | Quita una de tu bandeja |
| 🔒 `GET /listas-publicas` | Las listas que compartes: `{publicas: [{codigo, lista_id, nombre, actualizada}]}` |
| 🔒 `POST /listas-publicas` | `{lista_id, nombre, items: [{clave, tmdb_id, media_type, title, poster_path}]}` → `{codigo, ...}`; volver a subirla conserva el código |
| 🔒 `GET /listas-publicas/{codigo}` | Cualquier usuario: `{codigo, nombre, de, items}`. Los códigos fallidos cuentan para el freno de intentos |
| 🔒 `DELETE /listas-publicas/{codigo}` | Deja de compartirla (solo el dueño) |
| 🔒 `GET /llavero` | `{mias: {rd: {creada, prestada_a}}, prestadas: [{servicio, de, desde}]}` |
| 🔒 `POST /llavero/rd` | `{client_id, client_secret, refresh_token, access_token, expira}`: la vinculación para prestar |
| 🔒 `DELETE /llavero/rd` | Borra esa vinculación y todos sus préstamos |
| 🔒 `POST /llavero/rd/prestamos` | `{nombre}`: presta a un amigo |
| 🔒 `DELETE /llavero/rd/prestamos/{nombre}` | Deja de prestarle |
| 🔒 `GET /llavero/rd/token[?renovar=1]` | Para quien recibe: `{access_token, expira, de}` |

Cada equipo recibe su propio token y en la base solo se guarda su sha256.
Después de 10 intentos fallidos de login o de invitación desde una IP, responde
429 durante 15 minutos.

### Bóveda

Claves de API, ajustes y vinculaciones (Real-Debrid, OpenSubtitles) de cada
usuario, para que entrar con la cuenta en un Kutral nuevo lo deje igual que el
de siempre. Kutral la cifra antes de subirla con una llave que saca de la
contraseña (Argon2id + ChaCha20-Poly1305): el servidor guarda un texto opaco.
Cambiar la contraseña (`admin.php` o el panel) cierra todas las sesiones y
borra la bóveda; el primer equipo que vuelve a entrar la sube de nuevo.

### Llavero

Quien presta vincula en Kutral un código de Real-Debrid aparte, solo para el
llavero; el servidor guarda esas credenciales (en `datos/kutral.sqlite`, por eso
`datos/` nunca debe quedar a la vista) y renueva el token una hora antes de que
venza. Quien recibe solo obtiene el `access_token`, nunca el `refresh_token`, y
lo guarda en memoria. Con `renovar=1` (Real-Debrid le rechazó el token) el
servidor renueva, como mucho una vez por minuto. Dejar de prestar, dejar de
ser amigos (corta en los dos sentidos) o borrar la vinculación anula en el acto
el token que tenían (`disable_access_token`); a los que siguen con préstamo se
les entrega uno nuevo. Para renovar, PHP necesita salir a internet
(`curl` o `allow_url_fopen`).

En `POST /sync` gana el cambio con `actualizado` (ms) más reciente. Si el
servidor ya tiene algo más nuevo, el resultado de ese cambio es
`{"ok": false, "vigente": {...}}` para que el equipo se ponga al día.
