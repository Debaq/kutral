<div align="center">

# Kütral

**A blazing-fast desktop catalog for movies and TV — powered by TMDb, wrapped in Tauri.**

[![Release](https://img.shields.io/github/v/release/Debaq/kutral?style=for-the-badge&color=ff5722)](https://github.com/Debaq/kutral/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg?style=for-the-badge)](LICENSE)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-24C8DB?style=for-the-badge&logo=tauri&logoColor=white)](https://tauri.app)
[![Frontend: SvelteKit](https://img.shields.io/badge/frontend-SvelteKit-FF3E00?style=for-the-badge&logo=svelte&logoColor=white)](https://kit.svelte.dev)
[![Backend: Rust](https://img.shields.io/badge/backend-Rust-000000?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Platform: Windows](https://img.shields.io/badge/platform-Windows-0078D6?style=for-the-badge&logo=windows&logoColor=white)](#downloads)

*Kütral* (Mapudungun for **fire**) is a lightweight, native desktop app that turns
your discovery flow into something fast, beautiful and offline-aware.

[Download](#downloads) · [Features](#features) · [Quick start](#quick-start) · [Architecture](#architecture) · [Roadmap](#roadmap)

---

</div>

## Why Kütral

Browser tabs lose your scroll position. Web catalogs leak your taste to ad
networks. Streaming-aggregator sites bury what you want under affiliate noise.

Kütral is a **single, native window** that talks to TMDb directly, caches
posters locally, remembers what you watched, and gets out of the way the rest
of the time. No accounts. No telemetry. No tabs.

## Features

| | |
|---|---|
| **Native everywhere** | Built on Tauri 2 — Rust core, web UI, ~10 MB installer, instant startup |
| **TMDb-backed catalog** | Discover, search and filter movies and series with the full TMDb metadata graph |
| **Smart trailers** | YouTube trailers embedded via `nocookie`, postMessage progress sync |
| **Cast and filmography** | Drill into any actor or director with a single click — full person view with credits |
| **Local poster cache** | Images downsized and hashed to disk on first view; subsequent loads are zero-network |
| **SQLite watch history** | Resume playback by `imdb_id` with millisecond precision, all stored locally |
| **Playability probe** | Each title is probed for a working stream URL before it shows up — no dead tiles |
| **Global hotkeys** | Back-navigation via global shortcuts, even when the window is unfocused |
| **Fullscreen + kiosk** | One-keystroke immersive mode for TV setups and HTPC use |
| **Zero accounts** | Bring your own TMDb API key — everything else is local |

## Downloads

Grab the latest Windows installer (`.msi`) or portable build (`.exe`) from the
[Releases page](https://github.com/Debaq/kutral/releases/latest).

Builds are produced automatically by GitHub Actions on every tagged release.
macOS and Linux targets are planned — see [Roadmap](#roadmap).

## Quick start

### Prerequisites

- [Node.js](https://nodejs.org/) ≥ 20
- [Rust](https://rustup.rs/) (stable)
- [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS
- A free [TMDb API key](https://www.themoviedb.org/settings/api)

### Run from source

```bash
git clone https://github.com/Debaq/kutral.git
cd kutral
npm install
npm run tauri dev
```

The app will open and prompt you for your TMDb API key on first launch.
The key is stored locally via `localStorage` and never leaves your machine.

### Build a release binary

```bash
npm run tauri build
```

Output lands in `src-tauri/target/release/bundle/`.

## Configuration

| Setting | Where | Purpose |
|---|---|---|
| `tmdb_key` | localStorage | TMDb v3 API key (set on first launch) |
| `trailer_src` | localStorage | `nocookie` (default) or `youtube` for trailer embeds |
| Watch progress | SQLite (`kutral.db`) | Per-`imdb_id` resume position and duration |

## Keyboard shortcuts

| Action | Shortcut |
|---|---|
| Back / close detail | `Esc`, `Backspace`, `Alt+Left` |
| Toggle fullscreen | `F11` |
| Search | `/` |
| Navigate grid | Arrow keys (when enabled) |

Shortcuts are registered through `tauri-plugin-global-shortcut`, so they fire
even if another window has focus during fullscreen playback.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  SvelteKit (adapter-static) — UI, routing, state            │
│  └── invoke() ──► Tauri IPC                                 │
├─────────────────────────────────────────────────────────────┤
│  Rust core (src-tauri/src/lib.rs)                           │
│  ├── tmdb_discover / search / detail / videos / person      │
│  ├── item_status        — has_imdb + has_trailer probe      │
│  ├── check_url          — HEAD + redirect-loss detection    │
│  ├── inspect_player     — playability diagnostic            │
│  ├── cache_image        — fetch + resize + sha256 to disk   │
│  └── plugins: sql (sqlite), global-shortcut, opener         │
└─────────────────────────────────────────────────────────────┘
```

### Key design decisions

- **`imdb_id` as a playability proxy** — rather than probing every external
  player on every render, we treat the presence of an IMDb ID on the TMDb
  payload as a strong signal that the title is reachable. Cuts API chatter by
  an order of magnitude.
- **Static frontend, dynamic backend** — the UI builds to a single static
  bundle (no SSR), so the Tauri webview boots in milliseconds. All
  network/disk work happens in Rust.
- **No abstraction over TMDb** — the Rust side speaks TMDb v3 natively. If
  TMDb ever goes down, the fallback path swaps to [Cinemeta](https://cinemeta.strem.io)
  (IMDb-native, no key required) with a single config flip.

## Project layout

```
kutral/
├── src/                     SvelteKit app (UI)
│   ├── routes/+page.svelte  Main catalog + detail views
│   └── app.html
├── src-tauri/               Rust core
│   ├── src/lib.rs           All #[tauri::command] handlers
│   ├── Cargo.toml
│   └── tauri.conf.json      Window, bundle, security config
├── .github/workflows/
│   └── release.yml          Tag v* → Windows .msi + .exe to Releases
├── Kutral.sh                Project management script (dev/build/release)
└── package.json
```

## Release process

1. Bump the version triplet (semver) in:
   - `package.json`
   - `src-tauri/Cargo.toml` (and `Cargo.lock`)
   - `src-tauri/tauri.conf.json`
2. Commit: `chore(release): vYYYY.M.N`
3. Tag: `git tag -a vYYYY.MM.N -m "..."` — the **tag** can use the CalVer
   `YYYY.MM.N` form; the **internal** version must stay strict semver.
4. `git push --tags` — GitHub Actions builds `.msi` + `.exe`, creates the
   release, attaches the binaries.

## Roadmap

- [ ] **Vera v2** — conversational recommendation wizard with adjustable depth
      and tone (see [`vera-v2.md`](vera-v2.md))
- [ ] **macOS + Linux builds** — Tauri targets are ready, CI matrix pending
- [ ] **Auto-updater** — Tauri updater channel wired through GitHub Releases
- [ ] **Bring-your-own player** — plug-in interface for external playback URLs
- [ ] **Group mode** — multi-profile recommendations for shared sessions

## Contributing

Issues and PRs welcome. For larger changes, open an issue first to align on
scope. The Rust side passes `cargo clippy --all-targets -- -D warnings`; the
TypeScript side passes `npm run check`. Please keep both clean.

## Acknowledgements

- [TMDb](https://www.themoviedb.org/) for the most generous open metadata API
  on the web
- [Tauri](https://tauri.app) for proving that desktop apps don't need a
  hundred-megabyte runtime
- [Cinemeta](https://cinemeta.strem.io) as the IMDb-native fallback
- The Mapuche people, whose word *Kütral* (fire) names this project

## License

[MIT](LICENSE) © Debaq
