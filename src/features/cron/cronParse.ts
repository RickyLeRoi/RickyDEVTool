// Calcolo della prossima esecuzione di un'espressione cron a 5 campi
// (min hour dom month dow), più i keyword @daily/@hourly/… — tutto lato client,
// senza dipendenze. Usato per dire "quando" un cron job scatterà.

const SPECIAL: Record<string, string> = {
  "@yearly": "0 0 1 1 *",
  "@annually": "0 0 1 1 *",
  "@monthly": "0 0 1 * *",
  "@weekly": "0 0 * * 0",
  "@daily": "0 0 * * *",
  "@midnight": "0 0 * * *",
  "@hourly": "0 * * * *",
};

// Espande un campo cron ("*", "*/5", "1-3", "1,15") nell'insieme dei valori.
function parseField(field: string, min: number, max: number): Set<number> | null {
  const out = new Set<number>();
  for (const part of field.split(",")) {
    const [rangePart, stepPart] = part.split("/");
    const step = stepPart ? parseInt(stepPart, 10) : 1;
    if (isNaN(step) || step <= 0) return null;
    let lo = min;
    let hi = max;
    if (rangePart === "*" || rangePart === "") {
      // intervallo pieno
    } else if (rangePart.includes("-")) {
      const [a, b] = rangePart.split("-").map((n) => parseInt(n, 10));
      lo = a;
      hi = b;
    } else {
      lo = hi = parseInt(rangePart, 10);
    }
    if (isNaN(lo) || isNaN(hi) || lo < min || hi > max || lo > hi) return null;
    for (let v = lo; v <= hi; v += step) out.add(v);
  }
  return out;
}

/** Prossima esecuzione a partire da `from` (default: adesso), o null se non parsabile. */
export function cronNextRun(expr: string, from = new Date()): Date | null {
  let e = expr.trim();
  if (e.startsWith("@")) {
    const mapped = SPECIAL[e.split(/\s+/)[0]];
    if (!mapped) return null; // @reboot & co. non hanno un "prossimo orario"
    e = mapped;
  }
  const f = e.split(/\s+/);
  if (f.length < 5) return null;

  const min = parseField(f[0], 0, 59);
  const hour = parseField(f[1], 0, 23);
  const dom = parseField(f[2], 1, 31);
  const mon = parseField(f[3], 1, 12);
  const dow = parseField(f[4], 0, 7);
  if (!min || !hour || !dom || !mon || !dow) return null;
  if (dow.has(7)) dow.add(0); // domenica = 0 e 7

  const domRestricted = f[2] !== "*";
  const dowRestricted = f[4] !== "*";

  const d = new Date(from.getTime());
  d.setSeconds(0, 0);
  d.setMinutes(d.getMinutes() + 1);

  // Cap: cerca fino a ~366 giorni avanti (copre ogni cadenza annuale).
  for (let i = 0; i < 366 * 24 * 60; i++) {
    if (mon.has(d.getMonth() + 1) && hour.has(d.getHours()) && min.has(d.getMinutes())) {
      const domOk = dom.has(d.getDate());
      const dowOk = dow.has(d.getDay());
      // Regola cron: se sia dom sia dow sono ristretti vale l'OR, altrimenti l'AND.
      const dayOk = domRestricted && dowRestricted ? domOk || dowOk : domOk && dowOk;
      if (dayOk) return new Date(d.getTime());
    }
    d.setMinutes(d.getMinutes() + 1);
  }
  return null;
}
