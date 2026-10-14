// Aviso de vencimiento de Real-Debrid: desde 7 días antes, una vez al día, y
// también ya vencido (así se entiende por qué dejó de funcionar). Solo para el
// RD propio: el que presta un amigo lo avisa el Kutral de ese amigo.

import { invoke } from "@tauri-apps/api/core";
import { notify } from "$lib/notifStore.svelte";

export const DIAS_ANTES = 7;
const DIA = 86_400;
// Fecha local (AAAA-MM-DD) del último aviso: uno por día aunque se abra la
// app varias veces.
const AVISADO = "rd_vence_avisado";
// La app puede quedar abierta días (TV box): se vuelve a mirar cada tanto.
const CADA = 3 * 3600_000;

type RdAccount = { account_type: string; premium: number; expiration: string };

/** Qué avisar con `premium` segundos restantes; null = todavía no toca. */
export function avisoVencimiento(
  tipo: string,
  premium: number,
): { titulo: string; cuerpo: string } | null {
  const renovar = "Renuévalo en real-debrid.com para seguir usando Descubrir con debrid.";
  if (tipo !== "premium" || premium <= 0) {
    return { titulo: "Tu Real-Debrid no tiene premium", cuerpo: renovar };
  }
  const dias = Math.ceil(premium / DIA);
  if (dias > DIAS_ANTES) return null;
  const cuando = premium < DIA ? "hoy" : dias === 1 ? "mañana" : `en ${dias} días`;
  return { titulo: `Tu Real-Debrid vence ${cuando}`, cuerpo: renovar };
}

function hoy(): string {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

async function revisar(): Promise<void> {
  try {
    if (localStorage.getItem(AVISADO) === hoy()) return;
    if (!(await invoke<boolean>("rd_creds_propio"))) return;
    const a = await invoke<RdAccount>("rd_account");
    const aviso = avisoVencimiento(a.account_type, a.premium);
    if (!aviso) return;
    notify("warn", aviso.titulo, aviso.cuerpo);
    localStorage.setItem(AVISADO, hoy());
  } catch (e) {
    // Sin red o RD caído: se reintenta en la próxima vuelta.
    console.warn("[rd] aviso de vencimiento:", e);
  }
}

let iniciado = false;

export function iniciarAvisoRd(): void {
  if (iniciado) return;
  iniciado = true;
  void revisar();
  setInterval(() => void revisar(), CADA);
}
