#!/usr/bin/env python3
"""Lista de sitios vivos a partir de Balandro (addon de Kodi en español).

Balandro lo mantienen humanos: cada 2-4 días publican un parche con los
canales que arreglaron. De ahí sacamos SOLO datos, sin ejecutar su código:

  - channels/<id>.json → activo, idiomas, categorías y la nota humana.
  - channels/<id>.py   → `host = '...'` (dominio vigente) y `ant_hosts = [...]`
                          (dominios muertos), leídos con ast.parse: el archivo
                          se analiza como texto, nunca se importa.

Fuentes oficiales (cuenta repobal en GitHub):
  - repobal/base: el addon completo (zip versionado, ver addons.xml).
  - repobal/fix:  updates.zip con los archivos parcheados, encima del base.

Salida: JSON con cada canal + sha256 de su .py, para detectar cuándo cambia
un canal que Kütral usa (VIGILADOS) y avisar que hay que revisar nuestro
scraper. Uso:

  fuentes_balandro.py --salida balandro.json [--previo balandro.json] [--cambios cambios.md]
"""

import argparse
import ast
import datetime
import hashlib
import io
import json
import re
import sys
import urllib.request
import warnings
import zipfile

BASE = "https://raw.githubusercontent.com/repobal/base/main/"
FIX = "https://raw.githubusercontent.com/repobal/fix/main/updates.zip"
UA = {"User-Agent": "kutral-fuentes/1.0"}

# Canales de Balandro equivalentes a scrapers propios de Kütral: si cambian,
# los humanos de Balandro arreglaron algo y conviene revisar el nuestro.
VIGILADOS = {
    "pelispanda": "src-tauri/src/pelispanda.rs",
    "hacktorrent": "src-tauri/src/pelispanda.rs",
    "elitetorrent": "src-tauri/src/sitios_es.rs",
    "grantorrent": "src-tauri/src/sitios_es.rs",
    "henaojaran": "src-tauri/src/hosters_es.rs",
    "animeflvone": "src-tauri/src/hosters_es.rs",
    "peliculaspro": "src-tauri/src/hosters_es.rs",
    "jkanime": "src-tauri/src/anime_web.rs",
}


def bajar(url):
    with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=120) as r:
        return r.read()


def archivos_balandro():
    """{ruta relativa dentro del addon: bytes}, base + parche encima."""
    addons = bajar(BASE + "addons.xml").decode("utf-8", "replace")
    m = re.search(r'id="plugin\.video\.balandro"[^>]*version="([^"]+)"', addons)
    if not m:
        sys.exit("no encontré la versión de Balandro en addons.xml")
    version = m.group(1)
    zbase = zipfile.ZipFile(io.BytesIO(bajar(
        f"{BASE}plugin.video.balandro/plugin.video.balandro-{version}.zip")))
    out = {}
    pref = "plugin.video.balandro/"
    for n in zbase.namelist():
        if n.startswith(pref) and not n.endswith("/"):
            out[n[len(pref):]] = zbase.read(n)
    zfix = zipfile.ZipFile(io.BytesIO(bajar(FIX)))
    parcheados = 0
    for n in zfix.namelist():
        if not n.endswith("/"):
            out[n] = zfix.read(n)
            parcheados += 1
    return version, parcheados, out


def dominios(py_src):
    """(host, [muertos]) desde las asignaciones de nivel módulo del .py."""
    host, muertos = "", []
    try:
        # Su código trae escapes viejos ("\d" sin r""): avisos que no importan.
        with warnings.catch_warnings():
            warnings.simplefilter("ignore", SyntaxWarning)
            arbol = ast.parse(py_src)
    except SyntaxError:
        return host, muertos
    for nodo in arbol.body:
        if not isinstance(nodo, ast.Assign) or len(nodo.targets) != 1:
            continue
        t = nodo.targets[0]
        if not isinstance(t, ast.Name):
            continue
        v = nodo.value
        if t.id == "host" and isinstance(v, ast.Constant) and isinstance(v.value, str):
            host = v.value.strip()
        elif t.id == "ant_hosts" and isinstance(v, (ast.List, ast.Tuple)):
            muertos = [e.value.strip() for e in v.elts
                       if isinstance(e, ast.Constant) and isinstance(e.value, str)]
    return host, muertos


def armar(version, parcheados, archivos):
    canales = {}
    for ruta, datos in sorted(archivos.items()):
        m = re.fullmatch(r"channels/([a-z0-9_]+)\.json", ruta)
        if not m:
            continue
        cid = m.group(1)
        try:
            info = json.loads(datos.decode("utf-8", "replace"))
        except ValueError:
            continue
        cats = info.get("categories", [])
        if "adults" in cats or "adults" in info.get("clusters", []):
            continue
        py = archivos.get(f"channels/{cid}.py", b"")
        host, muertos = dominios(py.decode("utf-8", "replace"))
        canales[cid] = {
            "nombre": info.get("name", cid),
            "activo": bool(info.get("active")),
            "idiomas": info.get("language", []),
            "categorias": cats,
            "nota": info.get("notes", ""),
            "host": host,
            "muertos": muertos,
            "sha": hashlib.sha256(py).hexdigest()[:16] if py else "",
        }
    return {
        "generado": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"),
        "origen": f"repobal/base v{version} + repobal/fix ({parcheados} archivos)",
        "version": version,
        "canales": canales,
    }


def cambios(previo, nuevo):
    """Líneas de aviso para los canales vigilados que cambiaron."""
    out = []
    for cid, nuestro in VIGILADOS.items():
        a = previo.get("canales", {}).get(cid)
        b = nuevo["canales"].get(cid)
        if not b:
            if a:
                out.append(f"- **{cid}**: desapareció de Balandro. Revisar `{nuestro}`.")
            continue
        if not a:
            continue
        motivos = []
        if a.get("sha") != b.get("sha"):
            motivos.append("cambió su código")
        if a.get("host") != b.get("host"):
            motivos.append(f"dominio {a.get('host')} → {b.get('host')}")
        if a.get("activo") != b.get("activo"):
            motivos.append("ahora " + ("activo" if b.get("activo") else "INACTIVO"))
        if motivos:
            out.append(f"- **{cid}** ({', '.join(motivos)}). Revisar `{nuestro}`."
                       + (f" Nota de Balandro: {b['nota']}" if b.get("nota") else ""))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--salida", required=True)
    ap.add_argument("--previo")
    ap.add_argument("--cambios")
    a = ap.parse_args()

    version, parcheados, archivos = archivos_balandro()
    nuevo = armar(version, parcheados, archivos)
    activos = sum(c["activo"] for c in nuevo["canales"].values())
    print(f"{nuevo['origen']}: {len(nuevo['canales'])} canales, {activos} activos")

    previo = {}
    if a.previo:
        try:
            with open(a.previo, encoding="utf-8") as f:
                previo = json.load(f)
        except (OSError, ValueError):
            previo = {}
    # Sin cambios en los canales, no se reescribe: así el commit diario solo
    # aparece cuando Balandro de verdad tocó algo.
    if previo.get("canales") == nuevo["canales"]:
        print("sin cambios")
    else:
        with open(a.salida, "w", encoding="utf-8") as f:
            json.dump(nuevo, f, ensure_ascii=False, indent=1, sort_keys=True)
            f.write("\n")
    avisos = cambios(previo, nuevo)
    if a.cambios and avisos:
        with open(a.cambios, "w", encoding="utf-8") as f:
            f.write(f"Balandro publicó cambios ({nuevo['origen']}) en canales que Kütral usa:\n\n")
            f.write("\n".join(avisos) + "\n\n")
            f.write("Los humanos de Balandro arreglaron algo: conviene revisar si nuestro "
                    "scraper necesita lo mismo. No copiar su código (GPL); ver qué cambió "
                    "en el sitio y adaptarlo.\n")
    for l in avisos:
        print(l)


if __name__ == "__main__":
    main()
