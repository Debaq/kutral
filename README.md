# Fuentes vivas (rama de datos)

Rama solo de datos: no tiene código de la app. La escribe el workflow
`fuentes-balandro.yml` de `main` una vez al día.

- `balandro.json`: sitios vivos según Balandro (addon de Kodi mantenido por
  humanos, cuenta `repobal` en GitHub). Por canal: activo, idiomas, categorías,
  nota, dominio vigente (`host`), dominios muertos (`muertos`) y un hash de su
  código para detectar cambios.

Kütral lo lee al buscar fuentes (`src-tauri/src/dominios.rs`) para usar el
dominio vigente de cada sitio sin sacar una versión nueva.

Solo se leen datos de Balandro: su código nunca se ejecuta ni se copia (GPL).
