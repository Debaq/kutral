# Encadenar capítulos al transmitir a la TV

Estado: **aprobado, sin implementar** (2026-11-07).

## Qué pasa hoy

- **PC:** al terminar sale `PostCreditos.svelte` con "Siguiente capítulo", solo con
  Enter. Nada arranca solo (decisión de diseño a propósito).
- **TV:** al terminar no hay pantalla de fin. `SourcePicker.svelte` (~l.1243) ve
  `tvPos >= tvDur - 90` y cierra → vuelve al catálogo.
- **Navegando con la TV sonando** (Esc): solo el seguimiento global de
  `cast.svelte.ts` (`muestrear`), que guarda progreso y suelta la TV.
- `destinoTv` siempre parte en `false` (`SourcePicker.svelte` ~l.234): el
  siguiente capítulo se abriría en el PC.

## Propuesta

1. Ajuste **"Encadenar capítulos en la TV"**, apagado por defecto.
2. Fin en la TV (motivo `FINISHED` o la regla de los 90 s) → mismo camino que
   `mpv:fin` en `+page.svelte`, con una marca "seguir en la TV".
3. Aviso con **cuenta regresiva de 10 s en el PC**, Esc cancela.
4. El capítulo nuevo se monta con `destinoTv = true` + `sourcesAutoplay`.
   - Pack de temporada → `desdePack` (mismo torrent, misma versión, rápido).
   - Llevar audio elegido (parche de cabecera LG) y buscar subs del capítulo nuevo.
5. **Solo encadena solo** si el siguiente es del mismo pack, o trae el idioma
   y no está bloqueado por DMCA. Si no: se detiene y avisa en el PC (regla
   "ofrecer antes de autoplay").

## Control remoto de la TV

En la TV no se puede mostrar nada propio (receptor Cast por defecto / DLNA): el
control solo da **pausa, adelantar, atrasar y detener**. Se usa eso como señal:

- **Detener con el control** durante la cuenta/arranque = cancelar
  (ya se detecta: `IDLE`/`SIN_APP`).
- Idea a evaluar: **pausar en los últimos ~90 s** (créditos) = "no encadenar".
  El estado `PAUSED` ya llega por `cast_status`.
- Idea a evaluar: **atrasar** al final = sigue viendo los créditos, no encadena.

## Riesgos / por probar

- La app Cast puede cerrarse al terminar (`SIN_APP`) → relanzarla; unos segundos
  de pantalla de inicio en la TV.
- DLNA no siempre informa `FINISHED` → depende de la regla de los 90 s.
- Google TV sin probar.
- Encadenar con el SourcePicker desmontado (navegando otra cosa) es lo más
  difícil. Alternativa simple: encadenar solo si los controles de la TV siguen
  abiertos.
