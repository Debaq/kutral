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
    crearPerfil,
    editarPerfil,
    borrarPerfil,
    type Perfil,
  } from "$lib/perfiles.svelte";

  const MAX = 10;

  let administrar = $state(false);
  /** Perfil en edición; "nuevo" = alta. */
  let editando = $state<Perfil | "nuevo" | null>(null);
  let nombre = $state("");
  let color = $state(COLORES[0]);
  let confirmarBorrar = $state(false);
  let error = $state("");
  let raiz = $state<HTMLElement | null>(null);

  function enfocar() {
    setTimeout(() => raiz && enfocarPrimero(raiz.querySelector(".zona") ?? raiz), 40);
  }

  $effect(() => {
    if (perfiles.abierta) {
      administrar = false;
      editando = null;
      enfocar();
    }
  });

  function inicial(n: string): string {
    return n.trim().charAt(0).toUpperCase() || "?";
  }

  function abrirEditor(p: Perfil | "nuevo") {
    editando = p;
    nombre = p === "nuevo" ? "" : p.nombre;
    color = p === "nuevo" ? (COLORES.find((c) => !perfiles.lista.some((x) => x.color === c)) ?? COLORES[0]) : p.color;
    confirmarBorrar = false;
    error = "";
    enfocar();
  }

  function cerrarEditor() {
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
      if (editando === "nuevo") await crearPerfil(nombre, color);
      else if (editando) await editarPerfil(editando.id, nombre, color);
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

  function tocar(p: Perfil) {
    if (administrar) abrirEditor(p);
    else void elegirPerfil(p.id);
  }

  function onKey(e: KeyboardEvent) {
    if (!perfiles.abierta) return;
    e.stopPropagation();
    const enInput = (e.target as HTMLElement | null)?.tagName === "INPUT";
    switch (e.key) {
      case "Escape":
      case "Backspace":
        if (enInput && e.key === "Backspace") return;
        e.preventDefault();
        if (editando) cerrarEditor();
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

{#if perfiles.abierta}
  <div class="fondo" bind:this={raiz} data-section="perfiles">
    {#if !editando}
      <h1>{administrar ? "Administrar perfiles" : "¿Quién está viendo?"}</h1>
      <div class="zona fila">
        {#each perfiles.lista as p (p.id)}
          <button data-nav class="perfil" class:actual={p.id === perfiles.actual} onclick={() => tocar(p)}>
            <span class="avatar" style:background={p.color}>
              {inicial(p.nombre)}
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
    {:else}
      <h1>{editando === "nuevo" ? "Nuevo perfil" : "Editar perfil"}</h1>
      <div class="zona editor">
        <span class="avatar grande" style:background={color}>{inicial(nombre)}</span>
        <input
          data-nav
          bind:value={nombre}
          maxlength="40"
          placeholder="Nombre"
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
        <div class="acciones">
          <button data-nav class="btn principal" onclick={guardar}>Guardar</button>
          <button data-nav class="btn" onclick={cerrarEditor}>Cancelar</button>
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
