<script lang="ts">
  // "¿Quién está viendo?": al abrir la app con más de un perfil, o desde el
  // avatar del encabezado. Aquí también se crean, editan y borran perfiles.
  //
  // Todo con flechas y Enter (TV). Mientras está abierta, el teclado es suyo:
  // nada llega a la página de abajo.
  import { navegar, enfocarPrimero } from "$lib/nav";
  import {
    perfiles,
    COLORES,
    PRINCIPAL,
    elegirPerfil,
    cerrarPerfiles,
    marcarConfigurado,
    perfilDe,
    crearPerfil,
    editarPerfil,
    borrarPerfil,
    type Perfil,
  } from "$lib/perfiles.svelte";
  import ElegirCarita from "$lib/ElegirCarita.svelte";
  import { page } from "$app/state";
  import { untrack } from "svelte";

  const MAX = 10;

  let administrar = $state(false);
  /** Perfil en edición; "nuevo" = alta. */
  let editando = $state<Perfil | "nuevo" | null>(null);
  let nombre = $state("");
  let color = $state(COLORES[0]);
  let avatar = $state("");
  let infantil = $state(false);
  /** Eligiendo la carita (dentro del editor). */
  let carita = $state(false);
  // Caritas que no cargaron (sin red, link muerto): va la inicial.
  let rotas = $state(new Set<string>());
  let confirmarBorrar = $state(false);
  let error = $state("");
  let raiz = $state<HTMLElement | null>(null);

  function enfocar() {
    setTimeout(() => raiz && enfocarPrimero(raiz.querySelector(".zona") ?? raiz), 40);
  }

  // El asistente de la primera vez va antes: la bienvenida del perfil sale
  // al terminarlo.
  const visible = $derived(perfiles.abierta && !page.url.pathname.startsWith("/bienvenida"));
  const bienvenida = $derived(perfiles.configurar != null);

  // Solo al abrirse: recargar la lista (guardar un perfil) no reabre nada.
  $effect(() => {
    if (!visible) return;
    untrack(() => {
      administrar = false;
      editando = null;
      const p = perfiles.configurar ? perfilDe(perfiles.configurar) : null;
      if (p) {
        abrirEditor(p);
        nombre = "";
      } else enfocar();
    });
  });

  function inicial(n: string): string {
    return n.trim().charAt(0).toUpperCase() || "?";
  }

  function abrirEditor(p: Perfil | "nuevo") {
    editando = p;
    nombre = p === "nuevo" ? "" : p.nombre;
    color = p === "nuevo" ? (COLORES.find((c) => !perfiles.lista.some((x) => x.color === c)) ?? COLORES[0]) : p.color;
    avatar = p === "nuevo" ? "" : p.avatar || "";
    infantil = p === "nuevo" ? false : !!p.infantil;
    carita = false;
    confirmarBorrar = false;
    error = "";
    enfocar();
  }

  function cerrarEditor() {
    // Bienvenida: no hay más perfiles que elegir, se cierra todo.
    if (bienvenida) {
      marcarConfigurado();
      editando = null;
      cerrarPerfiles();
      return;
    }
    editando = null;
    error = "";
    enfocar();
  }

  async function guardar() {
    if (!nombre.trim()) {
      error = "Ponle un nombre.";
      return;
    }
    try {
      const datos = { nombre, color, avatar, infantil };
      if (editando === "nuevo") await crearPerfil(datos);
      else if (editando) await editarPerfil(editando.id, datos);
      cerrarEditor();
    } catch (e) {
      error = String(e);
    }
  }

  async function borrar() {
    if (!editando || editando === "nuevo") return;
    try {
      await borrarPerfil(editando.id);
      cerrarEditor();
    } catch (e) {
      error = String(e);
    }
  }

  function elegirCarita(url: string) {
    avatar = url;
    carita = false;
    enfocar();
  }

  function conCarita(url: string): boolean {
    return !!url && !rotas.has(url);
  }

  function rota(url: string) {
    rotas = new Set(rotas).add(url);
  }

  function tocar(p: Perfil) {
    if (administrar) abrirEditor(p);
    else void elegirPerfil(p.id);
  }

  function onKey(e: KeyboardEvent) {
    if (!visible) return;
    e.stopPropagation();
    // Texto (nombre, buscador de caritas): las flechas y el borrar son suyos.
    const t = e.target as HTMLInputElement | null;
    const enInput = t?.tagName === "INPUT" && t.type !== "checkbox";
    switch (e.key) {
      case "Escape":
      case "Backspace":
        if (enInput && e.key === "Backspace") return;
        e.preventDefault();
        if (carita) {
          carita = false;
          enfocar();
        } else if (editando) cerrarEditor();
        else if (administrar) administrar = false;
        else cerrarPerfiles();
        break;
      case "ArrowLeft":
      case "ArrowRight":
        if (enInput) return;
      // fallthrough
      case "ArrowUp":
      case "ArrowDown":
        e.preventDefault();
        if (raiz) navegar(e.key.replace("Arrow", "").toLowerCase() as "up" | "down" | "left" | "right", raiz);
        break;
    }
  }
</script>

<svelte:window onkeydowncapture={onKey} />

{#if visible}
  <div class="fondo" bind:this={raiz} data-section="perfiles">
    {#if !editando}
      <h1>{administrar ? "Administrar perfiles" : "¿Quién está viendo?"}</h1>
      <div class="zona fila">
        {#each perfiles.lista as p (p.id)}
          <button data-nav class="perfil" class:actual={p.id === perfiles.actual} onclick={() => tocar(p)}>
            <span class="avatar" style:background={p.color}>
              {#if conCarita(p.avatar)}
                <img src={p.avatar} alt="" onerror={() => rota(p.avatar)} />
              {:else}
                {inicial(p.nombre)}
              {/if}
              {#if p.infantil}<span class="nino" title="Perfil de niños">🧸</span>{/if}
              {#if administrar}<span class="lapiz">✎</span>{/if}
            </span>
            <span class="nombre">{p.nombre}</span>
          </button>
        {/each}
        {#if perfiles.lista.length < MAX}
          <button data-nav class="perfil" onclick={() => abrirEditor("nuevo")}>
            <span class="avatar nuevo">＋</span>
            <span class="nombre">Agregar</span>
          </button>
        {/if}
      </div>
      <div class="acciones">
        <button data-nav class="btn" onclick={() => (administrar = !administrar)}>
          {administrar ? "Listo" : "Administrar perfiles"}
        </button>
        {#if perfiles.elegido}
          <button data-nav class="btn" onclick={cerrarPerfiles}>Volver</button>
        {/if}
      </div>
      <p class="nota">
        Cada perfil tiene su historial, sus favoritos y sus listas. Las descargas y
        Real-Debrid son del equipo: los usan todos.
      </p>
    {:else if carita}
      <h1>Elige una carita</h1>
      <ElegirCarita onElegir={elegirCarita} />
    {:else}
      <h1>{bienvenida ? "¡Hola! ¿Quién eres?" : editando === "nuevo" ? "Nuevo perfil" : "Editar perfil"}</h1>
      {#if bienvenida}
        <p class="nota">Ponle tu nombre y elige una carita. Después puedes sumar perfiles para el resto de la casa.</p>
      {/if}
      <div class="zona editor">
        <button data-nav class="perfil" onclick={() => (carita = true)} title="Cambiar la carita">
          <span class="avatar grande" style:background={color}>
            {#if conCarita(avatar)}
              <img src={avatar} alt="" onerror={() => rota(avatar)} />
            {:else}
              {inicial(nombre)}
            {/if}
            <span class="lapiz suave">✎</span>
          </span>
          <span class="nombre">{avatar ? "Cambiar carita" : "Elegir carita"}</span>
        </button>
        <input
          data-nav
          bind:value={nombre}
          maxlength="40"
          placeholder={bienvenida ? "Tu nombre" : "Nombre"}
          onkeydown={(e) => e.key === "Enter" && void guardar()}
        />
        <div class="colores">
          {#each COLORES as c (c)}
            <button
              data-nav
              class="color"
              class:elegido={c === color}
              style:background={c}
              aria-label="Color {c}"
              onclick={() => (color = c)}
            ></button>
          {/each}
        </div>
        <label class="nino-fila">
          <input
            data-nav
            type="checkbox"
            bind:checked={infantil}
            onkeydown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                infantil = !infantil;
              }
            }}
          />
          <span>
            <strong>🧸 Perfil de niños</strong>
            <small>El catálogo muestra solo lo apto para ellos: películas hasta PG, series infantiles o familiares y anime sin contenido adulto.</small>
          </span>
        </label>
        <div class="acciones">
          <button data-nav class="btn principal" onclick={guardar}>Guardar</button>
          <button data-nav class="btn" onclick={cerrarEditor}>{bienvenida ? "Ahora no" : "Cancelar"}</button>
          {#if editando !== "nuevo" && editando.id !== PRINCIPAL}
            {#if confirmarBorrar}
              <button data-nav class="btn peligro" onclick={borrar}>Sí, borrar todo lo suyo</button>
            {:else}
              <button data-nav class="btn" onclick={() => (confirmarBorrar = true)}>Borrar perfil</button>
            {/if}
          {/if}
        </div>
        {#if confirmarBorrar}
          <p class="nota">
            Se borran su historial, sus favoritos y sus listas, en todos tus equipos.
          </p>
        {/if}
        {#if error}<p class="error">{error}</p>{/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .fondo {
    position: fixed;
    inset: 0;
    z-index: 2000;
    background: radial-gradient(ellipse at 50% 30%, #1a1a24 0%, #0d0d12 70%);
    color: #e6e6ec;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 34px;
    padding: 40px;
  }
  h1 {
    margin: 0;
    font-size: 34px;
    font-weight: 800;
    letter-spacing: 0.5px;
  }
  .fila {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 34px;
    max-width: 1100px;
  }
  .perfil {
    background: transparent;
    border: 0;
    color: #9a9aa4;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    cursor: pointer;
    padding: 6px;
    border-radius: 14px;
  }
  .avatar {
    position: relative;
    width: 120px;
    height: 120px;
    border-radius: 22px;
    display: grid;
    place-items: center;
    font-size: 52px;
    font-weight: 800;
    color: #0d0d12;
    transition: transform 0.12s;
    outline: 4px solid transparent;
    outline-offset: 4px;
  }
  .avatar img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    object-position: top;
    border-radius: inherit;
  }
  .nino {
    position: absolute;
    right: -8px;
    bottom: -8px;
    font-size: 30px;
    line-height: 1;
    filter: drop-shadow(0 2px 3px rgba(0, 0, 0, 0.6));
  }
  .lapiz.suave {
    opacity: 0;
    transition: opacity 0.12s;
  }
  .perfil:focus .lapiz.suave,
  .perfil:hover .lapiz.suave {
    opacity: 1;
  }
  .nino-fila {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    max-width: 440px;
    cursor: pointer;
  }
  .nino-fila input {
    width: 22px;
    height: 22px;
    padding: 0;
    margin-top: 2px;
    accent-color: #f5c518;
  }
  .nino-fila span {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .nino-fila small {
    color: #8a8a94;
    font-size: 12px;
    line-height: 1.4;
  }
  .avatar.nuevo {
    background: #1c1c26;
    color: #9a9aa4;
    border: 2px dashed #3a3a46;
  }
  .avatar.grande {
    width: 140px;
    height: 140px;
    font-size: 60px;
  }
  .lapiz {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    background: rgba(13, 13, 18, 0.55);
    color: #fff;
    font-size: 40px;
    border-radius: 22px;
  }
  .perfil.actual .avatar {
    outline-color: rgba(255, 255, 255, 0.35);
  }
  .perfil:focus,
  .perfil:focus-visible,
  .perfil:hover {
    outline: none;
    color: #fff;
  }
  .perfil:focus .avatar,
  .perfil:focus-visible .avatar,
  .perfil:hover .avatar {
    outline-color: #f5c518;
    transform: scale(1.06);
  }
  .nombre {
    font-size: 16px;
    font-weight: 700;
  }
  .acciones {
    display: flex;
    gap: 12px;
    flex-wrap: wrap;
    justify-content: center;
  }
  .btn {
    background: transparent;
    border: 1px solid #3a3a46;
    color: #c8c8d0;
    border-radius: 8px;
    padding: 10px 22px;
    font-size: 14px;
    font-weight: 700;
    letter-spacing: 0.4px;
    cursor: pointer;
  }
  .btn.principal {
    background: #f5c518;
    border-color: #f5c518;
    color: #0d0d12;
  }
  .btn.peligro {
    background: rgba(220, 38, 38, 0.22);
    border-color: #dc2626;
    color: #fff;
  }
  .btn:focus,
  .btn:focus-visible,
  input:focus {
    outline: none;
    border-color: #f5c518;
    box-shadow: 0 0 0 3px rgba(245, 197, 24, 0.3);
  }
  .editor {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 20px;
  }
  input {
    width: 320px;
    background: #15151c;
    border: 1px solid #3a3a46;
    color: #fff;
    border-radius: 10px;
    padding: 12px 16px;
    font-size: 18px;
    text-align: center;
  }
  .colores {
    display: flex;
    gap: 12px;
  }
  .color {
    width: 38px;
    height: 38px;
    border-radius: 50%;
    border: 3px solid transparent;
    cursor: pointer;
  }
  .color.elegido {
    border-color: #fff;
  }
  .color:focus,
  .color:focus-visible {
    outline: 3px solid #f5c518;
    outline-offset: 3px;
  }
  .nota {
    margin: 0;
    max-width: 520px;
    text-align: center;
    font-size: 13px;
    color: #6a6a74;
    line-height: 1.5;
  }
  .error {
    margin: 0;
    color: #ff7373;
    font-size: 13px;
  }
</style>
