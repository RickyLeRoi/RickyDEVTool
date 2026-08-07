// 20260807 RG costanti e default di aspetto: geometrie degli SVG, colori delle serie,
// classi e attributi che il tema scrive sul DOM. Tutto ciò che non è aspetto sta in
// constants.ts (valori) o defaults.ts (valori di partenza). I colori restano variabili CSS:
// la palette vive in styles.css, qui si scrive solo quale variabile usa quale serie.

/* ---------- tema ---------- */

export const THEME_ATTRIBUTE = "data-theme";
export const THEME_TRANSITION_CLASS = "theme-transition";
export const THEME_TRANSITION_MS = 1000;
export const THEME_BG_CSS_VAR = "--bg";
// serve alla meta theme-color prima che il CSS sia applicato.
export const THEME_COLOR_FALLBACK = "#16181d";

/* ---------- colori delle serie ---------- */

export const SERIES_COLORS = {
  cpu: "var(--accent)",
  mem: "var(--accent2)",
  disk: "var(--ok)",
} as const;

/* ---------- sparkline ---------- */

export const SPARKLINE = {
  max: 100,
  width: 180,
  height: 28,
  stroke: SERIES_COLORS.cpu,
  strokeWidth: "1.5",
  // margine verticale che tiene il tratto dentro il viewBox.
  inset: 1,
} as const;

// nel pannello vitali le sparkline sono più basse: la riga è compatta.
export const VITALS_SPARKLINE = { width: 180, height: 24 } as const;

/* ---------- grafico dello storico metriche ---------- */

export const METRICS_CHART = {
  width: 560,
  height: 160,
  padLeft: 6,
  padRight: 10,
  padTop: 8,
  padBottom: 6,
  gridColor: "var(--border)",
  gridStrokeWidth: "1",
  seriesStrokeWidth: "1.5",
  gridLinePcts: [0, 25, 50, 75, 100],
  yAxisLabelPcts: [100, 50, 0],
} as const;

/* ---------- QR di pairing ---------- */

export const QR_SIZE_PX = 220;
