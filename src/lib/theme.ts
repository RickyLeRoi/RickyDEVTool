export type Theme = "auto" | "light" | "dark";

const KEY = "rdt-theme";

export function getTheme(): Theme {
  const stored = localStorage.getItem(KEY);
  return stored === "light" || stored === "dark" ? stored : "auto";
}

/**
 * "auto" = nessun attributo, la CSS segue prefers-color-scheme.
 * "light"/"dark" = attributo esplicito che vince sulla media query.
 */
export function applyTheme(theme: Theme) {
  localStorage.setItem(KEY, theme);
  const root = document.documentElement;
  if (theme === "auto") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", theme);
  // Allinea la barra del titolo mobile al fondo del tema attivo.
  const bg = getComputedStyle(root).getPropertyValue("--bg").trim();
  document.querySelector('meta[name="theme-color"]')?.setAttribute("content", bg || "#16181d");
}

/** Applica il tema salvato all'avvio, prima del render. */
export function initTheme() {
  applyTheme(getTheme());
}
