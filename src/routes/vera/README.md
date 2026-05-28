# Vera and Chill + Habla — prototipo

Prototipo navegable de los dos personajes de Kütral: **Vera** (rojo-naranjo brasa, elige qué ver en 60s) y **Habla** (azul celeste, te acompaña sin bloquear).

## Cómo correrlo

Desde la raíz del repo:

```bash
npm install
npm run dev
```

Abrir en el navegador: `http://localhost:5173/vera`.

> No requiere Tauri ni base de datos. Todo el flujo vive en cliente con un catálogo dummy en JSON. El resto de `Kütral` sigue funcionando aparte. La importación TMDb de admin sigue en `/vera/catalog`.

## Qué ver

Flujo completo end-to-end:

1. **Entrada** — marca grande, pósters drift, slogan `Tú pones el sillón. Yo propongo qué ver.`
2. **Contexto** — una pregunta: `Solo / Pareja / Con peques`. `Peques` activa filtro `familyFriendly` duro.
3. **Mazo** — 8 cartas (constante `CARTAS_MAZO`). Reaccionar por botón o arrastrar (⬅ rechazo, ⬆ ya la vi, ➡ me llama). Vera comenta en vivo.
4. **Tadán** — Vera devuelve los datos reales recogidos (contexto + rechazos + vistos + hora) y propone UNA peli. Botón `▶ Reproducir`. Abajo, link chico `no me convence` muestra su otra carta.
5. **Carta rara "hoy no hay match"** — si el score de la mejor opción no supera `UMBRAL_NO_MATCH`, Vera es honesta y manda a poner algo conocido.
6. **Reproducción / pausa / fin** — mock player. Pausar > 4s dispara la burbuja Habla. Terminar abre Habla con frase de cierre (a veces "joya" si el contexto es `solo`).

## Atajos de teclado (obligatorio — sin ratón)

Cada pantalla pone el foco automáticamente al cargar. Todo se controla con teclas:

| Pantalla | Tecla | Acción |
|---|---|---|
| Entrada | `Enter` / `Espacio` | Empezar |
| Contexto | `←` `→` `↑` `↓` | Mover selección |
| Contexto | `Enter` / `Espacio` | Confirmar |
| Contexto | `1` / `2` / `3` | Atajo directo |
| Mazo | `←` | No me llama |
| Mazo | `↑` | Ya la vi |
| Mazo | `→` | Me llama |
| Tadán | `Enter` | Reproducir |
| Tadán | `N` | No me convence |
| No-match / Fin | `Enter` | Volver a empezar |
| Reproduciendo | `Espacio` | Pausar |
| Pausa | `Espacio` | Reanudar |
| Reproduciendo / Pausa | `T` | Terminar |
| **Cualquier pantalla** | `Esc` | **Salir de Vera** (a `/`) |
| Habla visible | `Esc` | Cerrar Habla (no sale) |

## Habla

- Burbuja flotante abajo-izquierda. Siempre dismissible, nunca bloquea.
- Dos caras según contexto:
  - `solo`: Habla es compañía. `Llegaste. ¿Cómo te fue hoy?`
  - `pareja` / `peques`: Habla te empuja hacia los humanos. `¿Ya le contaste cómo te fue?`
- Frase joya rara solo en `solo`, probabilidad 45%, al cierre.
- Stub `comentarioEscenaFuturo()` deja sitio para enchufar LLM + subtítulos en pausa (sin tocar Vera).

## Stack y decisiones

- **SvelteKit 5 + Svelte runes + TypeScript.** Ya era el stack del repo, runes (`$state`, `$derived`) hacen el motor reactivo limpio sin stores externos.
- **Estático.** `+layout.ts` ya tiene `ssr = false`. El prototipo no carga Tauri ni SQL — funciona con `vite dev` puro y dentro de `tauri dev` igual.
- **Sin IA en Vera.** Tabla de pesos por gesto, filtros duros por contexto, score por género + tono + bonus rating + sesgo nocturno. Las 3 cartas internas (segura/distinta/sorpresa) salen por regla, no por modelo.
- **Sin LLM en Habla** todavía. Bancos de plantillas con rotación (memoria de últimas N frases por grupo para no repetir). Stub `comentarioEscenaFuturo()` marcado con `// FUTURO`.
- **Pósters dummy** = rectángulos de color hex. Suficiente para sentir el ritmo del mazo sin pelear con assets externos.
- **Persistencia** = ninguna en este prototipo. Cada sesión arranca limpia. Cuando se integre, se puede colgar de SQLite (Tauri) o `localStorage`.

## Cosas que se ajustan editando una constante

`src/lib/vera-chill/config.ts`:

| constante | qué hace |
|---|---|
| `CARTAS_MAZO` | cuántas cartas en el mazo (8 por defecto) |
| `UMBRAL_NO_MATCH` | score mínimo de la segura para no caer en `no-match` |
| `HORA_NOCHE` | hora a partir de la cual Vera sesga hacia liviano |
| `DEMO_PAUSA_MS` | cuánto esperar en pausa para que Habla aparezca |
| `DEMO_PAUSA_MINIMA_MS` | pausa por debajo de esto no dispara Habla |
| `MEMORIA_FRASES` | últimas N frases que Vera no repite |

## Estructura

```
src/lib/vera/
  catalogo.json     ~30 películas dummy, editable
  config.ts         constantes ajustables
  tipos.ts          tipos del flujo
  motor.ts          recomendación determinista
  frases.ts         plantillas Vera + builder tadán dinámico
  import.ts         importador TMDb (admin, usado por /vera/catalog)
src/lib/habla/
  Habla.svelte      burbuja no bloqueante
  frases.ts         plantillas Habla por contexto + momento
src/routes/vera/
  +page.svelte      orquestador del flujo
  catalog/+page.svelte  admin de importación TMDb
  README.md         este archivo
```
