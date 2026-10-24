<script lang="ts">
  // Carita de un perfil: un personaje de anime (AniList) o, de películas y
  // series, el póster o la cara de alguien del reparto (TMDb no tiene imágenes
  // de personajes: sale el actor, con el nombre del personaje). Se guarda el
  // link a la imagen.
  //
  // Vive dentro de "¿Quién está viendo?", que maneja las flechas y Esc.
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { config } from "$lib/config.svelte";
  import { enfocarPrimero } from "$lib/nav";
  import type { PersonMini } from "$lib/tipos";

  let { onElegir }: { onElegir: (url: string) => void } = $props();

  type Opcion = { imagen: string; nombre: string; de: string };
  type Titulo = { id: number; mediaType: "movie" | "tv"; nombre: string; poster: string };

  const IMG = "https://image.tmdb.org/t/p/";

  let fuente = $state<"anime" | "peli">("anime");
  let texto = $state("");
  let cargando = $state(false);
  let error = $state("");
  let opciones = $state<Opcion[]>([]);
  let titulos = $state<Titulo[]>([]);
  /** Película o serie elegida: se muestran su póster y su reparto. */
  let titulo = $state<Titulo | null>(null);
  let raiz = $state<HTMLElement | null>(null);
  // La última búsqueda gana: escribir rápido dispara varias.
  let gen = 0;

  function enfocarResultados() {
    setTimeout(() => raiz && enfocarPrimero(raiz.querySelector(".grilla") ?? raiz), 40);
  }

  async function buscar() {
    const g = ++gen;
    error = "";
    titulo = null;
    opciones = [];
    titulos = [];
    if (fuente === "peli" && !texto.trim()) return;
    cargando = true;
    try {
      if (fuente === "anime") {
        const r = await invoke<Opcion[]>("anilist_personajes", { buscar: texto.trim() || null });
        if (g === gen) opciones = r;
      } else {
        const apiKey = config.tmdbKey;
        if (!apiKey) throw new Error("Falta la clave de TMDb en Configuración.");
        type Resp = { results: { id: number; title?: string; name?: string; poster_path?: string; popularity?: number }[] };
        const [pelis, series] = await Promise.all(
          (["movie", "tv"] as const).map((mt) =>
            invoke<Resp>("tmdb_search", { mediaType: mt, query: texto.trim(), page: 1, apiKey })
              .then((r) => r.results.map((x) => ({ ...x, mt })))
              .catch(() => []),
          ),
        );
        if (g !== gen) return;
        titulos = [...pelis, ...series]
          .filter((x) => x.poster_path)
          .sort((a, b) => (b.popularity ?? 0) - (a.popularity ?? 0))
          .slice(0, 18)
          .map((x) => ({
            id: x.id,
            mediaType: x.mt,
            nombre: x.title || x.name || "",
            poster: `${IMG}w342${x.poster_path}`,
          }));
      }
      if (g === gen && !opciones.length && !titulos.length) error = "No encontré nada con ese nombre.";
    } catch (e) {
      if (g === gen) error = e instanceof Error ? e.message : String(e);
    } finally {
      if (g === gen) cargando = false;
    }
  }

  async function abrirTitulo(t: Titulo) {
    const g = ++gen;
    titulo = t;
    error = "";
    cargando = true;
    try {
      const d = await invoke<{ cast?: PersonMini[] }>("tmdb_detail", {
        mediaType: t.mediaType,
        id: t.id,
        apiKey: config.tmdbKey,
      });
      if (g !== gen) return;
      opciones = [
        { imagen: t.poster, nombre: "Póster", de: t.nombre },
        ...(d.cast ?? [])
          .filter((p) => p.profile_path)
          .slice(0, 24)
          .map((p) => ({
            imagen: `${IMG}w185${p.profile_path}`,
            nombre: p.character || p.name,
            de: p.character ? p.name : t.nombre,
          })),
      ];
      enfocarResultados();
    } catch (e) {
      if (g === gen) error = String(e);
    } finally {
      if (g === gen) cargando = false;
    }
  }

  function cambiarFuente(f: "anime" | "peli") {
    if (f === fuente) return;
    fuente = f;
    void buscar();
  }

  // Con lo que se va escribiendo, sin tener que apretar Enter.
  let espera: ReturnType<typeof setTimeout> | null = null;
  function alEscribir() {
    if (espera) clearTimeout(espera);
    espera = setTimeout(() => void buscar(), 450);
  }

  onMount(() => {
    void buscar();
    return () => {
      if (espera) clearTimeout(espera);
    };
  });
</script>

<div class="carita" bind:this={raiz}>
  <div class="zona barra" data-section="carita-barra">
    <button data-nav class="tab" class:sel={fuente === "anime"} onclick={() => cambiarFuente("anime")}>Anime</button>
    <button data-nav class="tab" class:sel={fuente === "peli"} onclick={() => cambiarFuente("peli")}>
      Películas y series
    </button>
    <input
      data-nav
      bind:value={texto}
      oninput={alEscribir}
      onkeydown={(e) => {
        if (e.key === "Enter") {
          if (espera) clearTimeout(espera);
          void buscar().then(enfocarResultados);
        }
      }}
      placeholder={fuente === "anime" ? "Buscar personaje (Goku, Totoro…)" : "Buscar película o serie"}
    />
    <button data-nav class="tab" onclick={() => onElegir("")}>Sin carita</button>
  </div>

  {#if titulo}
    <p class="ayuda">
      {titulo.nombre}: el póster o alguien del reparto.
      <button data-nav class="link" onclick={() => { titulo = null; opciones = []; void buscar(); }}>← Otros títulos</button>
    </p>
  {:else if fuente === "peli" && !texto.trim()}
    <p class="ayuda">Escribe una película o serie y elige su póster o a alguien del reparto.</p>
  {:else if fuente === "anime" && !texto.trim()}
    <p class="ayuda">Los personajes más queridos. Escribe para buscar otro.</p>
  {/if}

  <div class="zona grilla" data-section="carita-grilla">
    {#if fuente === "peli" && !titulo}
      {#each titulos as t (t.mediaType + t.id)}
        <button data-nav class="op titulo" onclick={() => void abrirTitulo(t)}>
          <img src={t.poster} alt="" loading="lazy" />
          <span class="nom">{t.nombre}</span>
        </button>
      {/each}
    {:else}
      {#each opciones as o (o.imagen)}
        <button data-nav class="op" onclick={() => onElegir(o.imagen)} title={o.de}>
          <img src={o.imagen} alt="" loading="lazy" />
          <span class="nom">{o.nombre}</span>
          {#if o.de}<span class="de">{o.de}</span>{/if}
        </button>
      {/each}
    {/if}
  </div>
  {#if cargando}<p class="ayuda">Buscando…</p>{/if}
  {#if error}<p class="error">{error}</p>{/if}
</div>

<style>
  .carita {
    width: min(1100px, 92vw);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 16px;
  }
  .barra {
    display: flex;
    gap: 10px;
    flex-wrap: wrap;
    justify-content: center;
  }
  .tab {
    background: transparent;
    border: 1px solid #3a3a46;
    color: #c8c8d0;
    border-radius: 8px;
    padding: 10px 18px;
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
  }
  .tab.sel {
    background: #f5c518;
    border-color: #f5c518;
    color: #0d0d12;
  }
  input {
    width: 300px;
    background: #15151c;
    border: 1px solid #3a3a46;
    color: #fff;
    border-radius: 10px;
    padding: 10px 14px;
    font-size: 15px;
  }
  .tab:focus,
  .tab:focus-visible,
  input:focus,
  .link:focus {
    outline: none;
    border-color: #f5c518;
    box-shadow: 0 0 0 3px rgba(245, 197, 24, 0.3);
  }
  .grilla {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(118px, 1fr));
    gap: 14px;
    width: 100%;
    max-height: 56vh;
    overflow-y: auto;
    padding: 6px;
  }
  .op {
    background: transparent;
    border: 0;
    color: #c8c8d0;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    cursor: pointer;
    padding: 4px;
    border-radius: 12px;
    min-width: 0;
  }
  .op img {
    width: 104px;
    height: 104px;
    object-fit: cover;
    object-position: top;
    border-radius: 18px;
    background: #1c1c26;
    outline: 3px solid transparent;
    outline-offset: 3px;
  }
  .op.titulo img {
    height: 150px;
    border-radius: 10px;
  }
  .op:focus,
  .op:hover {
    outline: none;
    color: #fff;
  }
  .op:focus img,
  .op:hover img {
    outline-color: #f5c518;
  }
  .nom,
  .de {
    max-width: 112px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
  }
  .nom {
    font-weight: 700;
  }
  .de {
    color: #6a6a74;
    font-size: 11px;
  }
  .ayuda {
    margin: 0;
    font-size: 13px;
    color: #8a8a94;
    text-align: center;
  }
  .link {
    background: none;
    border: 1px solid transparent;
    border-radius: 6px;
    color: #f5c518;
    font-weight: 700;
    cursor: pointer;
    margin-left: 8px;
  }
  .error {
    margin: 0;
    color: #ff7373;
    font-size: 13px;
  }
</style>
