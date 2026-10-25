# Sonido de entrada de Kütral: fósforo → llamarada → crepitar + acorde cálido.
import numpy as np, wave, math
SR = 48000
DUR = 3.0
N = int(SR * DUR)
t = np.arange(N) / SR
rng = np.random.default_rng(7)

def lp1(x, fc):
    """Pasa bajos de un polo; fc puede ser un arreglo (barrido)."""
    fc = np.broadcast_to(np.asarray(fc, float), x.shape)
    a = 1 - np.exp(-2 * np.pi * fc / SR)
    y = np.empty_like(x); s = 0.0
    for i in range(len(x)):
        s += a[i] * (x[i] - s); y[i] = s
    return y

def hp1(x, fc):
    return x - lp1(x, fc)

def env(t0, ataque, caida, forma=1.0):
    e = np.zeros(N)
    m = t >= t0
    tt = t[m] - t0
    e[m] = np.minimum(tt / max(ataque, 1e-4), 1.0) ** forma * np.exp(-np.maximum(tt - ataque, 0) / caida)
    return e

out = np.zeros(N)

# 1) Fósforo: raspón de ruido agudo con textura irregular (0.00–0.12 s)
ras = hp1(rng.standard_normal(N), 2500)
grano = (rng.random(N) < 0.35).astype(float)
grano = lp1(grano, 900)
out += 0.55 * ras * grano * env(0.0, 0.01, 0.045)

# 2) Llamarada: ruido con pasa bajos que se abre (whoosh) + golpe grave
ruido = rng.standard_normal(N)
corte = 150 + 2600 * env(0.10, 0.22, 0.35)
whoosh = lp1(lp1(ruido, corte), corte)
out += 1.6 * whoosh * env(0.10, 0.20, 0.45, 1.5)
f_golpe = 70 - 25 * np.clip((t - 0.12) / 0.4, 0, 1)
fase = 2 * np.pi * np.cumsum(f_golpe) / SR
out += 0.45 * np.sin(fase) * env(0.12, 0.03, 0.35)

# 3) Crepitar: chasquidos cortos, más densos al principio
n_chas = 38
for k in range(n_chas):
    t0 = 0.28 + (2.4 * (rng.random() ** 1.6))
    i0 = int(t0 * SR)
    largo = int(SR * rng.uniform(0.002, 0.007))
    if i0 + largo >= N:
        continue
    clic = rng.standard_normal(largo) * np.exp(-np.linspace(0, 6, largo))
    amp = rng.uniform(0.08, 0.32) * (1.0 - 0.6 * (t0 - 0.28) / 2.4)
    out[i0:i0 + largo] += amp * clic
out = out + 0.0  # (los chasquidos ya llevan su brillo)

# 4) Acorde cálido (Re mayor abierto) que crece con la llama
notas = [73.42, 110.0, 146.83, 185.0, 220.0, 293.66]
pad = np.zeros(N)
for j, f in enumerate(notas):
    for det in (-0.6, 0.0, 0.7):
        ff = f * (2 ** (det / 1200 * 8))
        ph = rng.uniform(0, 2 * np.pi)
        # Seno con un poco de 2.º y 3.er armónico: cálido, sin aspereza
        pad += (np.sin(2 * np.pi * ff * t + ph)
                + 0.25 * np.sin(4 * np.pi * ff * t + ph)
                + 0.08 * np.sin(6 * np.pi * ff * t + ph)) / (1 + 0.35 * j)
pad /= np.max(np.abs(pad))
e_pad = np.clip((t - 0.30) / 0.55, 0, 1) ** 2
e_pad *= np.exp(-np.maximum(t - 0.85, 0) / 1.1)
out += 0.42 * pad * e_pad
# Brillo de la llama sobre el acorde: una quinta alta que aparece y se va
out += 0.06 * np.sin(2 * np.pi * 440.0 * t) * np.clip((t - 0.5) / 0.6, 0, 1) * np.exp(-np.maximum(t - 1.1, 0) / 0.6)

# Ruido de fuego de fondo, suave
brasa = lp1(rng.standard_normal(N), 700)
out += 0.25 * brasa * np.clip((t - 0.3) / 0.4, 0, 1) * np.exp(-np.maximum(t - 1.0, 0) / 0.9)

# Final: fundido y normalizado
fade = np.clip((DUR - t) / 0.6, 0, 1) ** 2
out *= fade
out = hp1(out, 30)
out /= np.max(np.abs(out)) / 0.89
print("pico", np.max(np.abs(out)), "rms", np.sqrt(np.mean(out ** 2)))
for s in np.arange(0, DUR, 0.25):
    i = int(s * SR); j = int((s + 0.25) * SR)
    print(f"{s:4.2f}s rms {np.sqrt(np.mean(out[i:j]**2)):.3f}")
with wave.open("kutral-intro.wav", "wb") as w:
    w.setnchannels(1); w.setsampwidth(2); w.setframerate(SR)
    w.writeframes((out * 32767).astype("<i2").tobytes())
