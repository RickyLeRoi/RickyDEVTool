// Conversioni colore pure e testabili. Il picker lavora in HSV (quadrato
// saturazione/valore + slider tinta, lo standard degli eyedropper); i
// convertitori mostrano RGB/RGBA/HEX/HSL derivati dallo stesso RGBA.

export interface RGBA {
  r: number; // 0..255 interi
  g: number;
  b: number;
  a: number; // 0..1
}

const clamp = (x: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, x));
const r2 = (x: number) => Math.round(x);

export function rgbToHsv(r: number, g: number, b: number): { h: number; s: number; v: number } {
  r /= 255;
  g /= 255;
  b /= 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const d = max - min;
  let h = 0;
  if (d !== 0) {
    if (max === r) h = ((g - b) / d) % 6;
    else if (max === g) h = (b - r) / d + 2;
    else h = (r - g) / d + 4;
    h *= 60;
    if (h < 0) h += 360;
  }
  const s = max === 0 ? 0 : d / max;
  return { h, s: s * 100, v: max * 100 };
}

export function hsvToRgb(h: number, s: number, v: number): { r: number; g: number; b: number } {
  s /= 100;
  v /= 100;
  h = ((h % 360) + 360) % 360;
  const c = v * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = v - c;
  let rp = 0,
    gp = 0,
    bp = 0;
  if (h < 60) [rp, gp, bp] = [c, x, 0];
  else if (h < 120) [rp, gp, bp] = [x, c, 0];
  else if (h < 180) [rp, gp, bp] = [0, c, x];
  else if (h < 240) [rp, gp, bp] = [0, x, c];
  else if (h < 300) [rp, gp, bp] = [x, 0, c];
  else [rp, gp, bp] = [c, 0, x];
  return { r: r2((rp + m) * 255), g: r2((gp + m) * 255), b: r2((bp + m) * 255) };
}

export function rgbToHsl(r: number, g: number, b: number): { h: number; s: number; l: number } {
  r /= 255;
  g /= 255;
  b /= 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const d = max - min;
  const l = (max + min) / 2;
  let h = 0;
  let s = 0;
  if (d !== 0) {
    s = d / (1 - Math.abs(2 * l - 1));
    if (max === r) h = ((g - b) / d) % 6;
    else if (max === g) h = (b - r) / d + 2;
    else h = (r - g) / d + 4;
    h *= 60;
    if (h < 0) h += 360;
  }
  return { h, s: s * 100, l: l * 100 };
}

export function hslToRgb(h: number, s: number, l: number): { r: number; g: number; b: number } {
  s /= 100;
  l /= 100;
  h = ((h % 360) + 360) % 360;
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = l - c / 2;
  let rp = 0,
    gp = 0,
    bp = 0;
  if (h < 60) [rp, gp, bp] = [c, x, 0];
  else if (h < 120) [rp, gp, bp] = [x, c, 0];
  else if (h < 180) [rp, gp, bp] = [0, c, x];
  else if (h < 240) [rp, gp, bp] = [0, x, c];
  else if (h < 300) [rp, gp, bp] = [x, 0, c];
  else [rp, gp, bp] = [c, 0, x];
  return { r: r2((rp + m) * 255), g: r2((gp + m) * 255), b: r2((bp + m) * 255) };
}

const hex2 = (n: number) => clamp(r2(n), 0, 255).toString(16).padStart(2, "0");

export function toHex({ r, g, b, a }: RGBA, withAlpha = false): string {
  const base = `#${hex2(r)}${hex2(g)}${hex2(b)}`;
  if (withAlpha || a < 1) return `${base}${hex2(a * 255)}`;
  return base;
}

export function toRgbString({ r, g, b }: RGBA): string {
  return `rgb(${r2(r)}, ${r2(g)}, ${r2(b)})`;
}

export function toRgbaString({ r, g, b, a }: RGBA): string {
  return `rgba(${r2(r)}, ${r2(g)}, ${r2(b)}, ${round2(a)})`;
}

export function toHslString(c: RGBA): string {
  const { h, s, l } = rgbToHsl(c.r, c.g, c.b);
  const a = c.a < 1 ? `, ${round2(c.a)}` : "";
  const fn = c.a < 1 ? "hsla" : "hsl";
  return `${fn}(${r2(h)}, ${r2(s)}%, ${r2(l)}%${a})`;
}

function round2(x: number): number {
  return Math.round(x * 100) / 100;
}

/** Interpreta una stringa colore in RGBA. Accetta #hex (3/4/6/8), rgb()/rgba(),
 *  hsl()/hsla(). `null` se non riconosciuta. */
export function parseColor(input: string): RGBA | null {
  const s = input.trim().toLowerCase();
  if (!s) return null;

  if (s.startsWith("#")) {
    const hex = s.slice(1);
    const expand = (h: string) =>
      h
        .split("")
        .map((c) => c + c)
        .join("");
    let full: string | null = null;
    if (hex.length === 3 || hex.length === 4) full = expand(hex);
    else if (hex.length === 6 || hex.length === 8) full = hex;
    if (!full || !/^[0-9a-f]+$/.test(full)) return null;
    const r = parseInt(full.slice(0, 2), 16);
    const g = parseInt(full.slice(2, 4), 16);
    const b = parseInt(full.slice(4, 6), 16);
    const a = full.length === 8 ? parseInt(full.slice(6, 8), 16) / 255 : 1;
    return { r, g, b, a };
  }

  const fn = s.match(/^(rgba?|hsla?)\s*\(([^)]+)\)$/);
  if (fn) {
    const kind = fn[1];
    const parts = fn[2].split(/[,\s/]+/).filter(Boolean);
    if (parts.length < 3) return null;
    const num = (p: string) => parseFloat(p);
    const alpha = parts[3] != null ? clamp(num(parts[3].replace("%", "")) / (parts[3].includes("%") ? 100 : 1), 0, 1) : 1;
    if (kind.startsWith("rgb")) {
      const conv = (p: string) => (p.includes("%") ? (num(p) / 100) * 255 : num(p));
      const r = clamp(r2(conv(parts[0])), 0, 255);
      const g = clamp(r2(conv(parts[1])), 0, 255);
      const b = clamp(r2(conv(parts[2])), 0, 255);
      if ([r, g, b].some((v) => Number.isNaN(v))) return null;
      return { r, g, b, a: alpha };
    } else {
      const h = num(parts[0]);
      const sl = num(parts[1].replace("%", ""));
      const l = num(parts[2].replace("%", ""));
      if ([h, sl, l].some((v) => Number.isNaN(v))) return null;
      const { r, g, b } = hslToRgb(h, sl, l);
      return { r, g, b, a: alpha };
    }
  }
  return null;
}
