export type Theme = "auto" | "light" | "dark";

const KEY = "rdt-theme";
const TRANSITION_MS = 1000;
let transitionTimer: ReturnType<typeof setTimeout> | undefined;

export function getTheme(): Theme {
  const stored = localStorage.getItem(KEY);
  return stored === "light" || stored === "dark" ? stored : "auto";
}

/**
 * "auto" = nessun attributo, la CSS segue prefers-color-scheme.
 * "light"/"dark" = attributo esplicito che vince sulla media query.
 * Con animate=true la commutazione dei colori sfuma in ~1s.
 */
export function applyTheme(theme: Theme, animate = false) {
  localStorage.setItem(KEY, theme);
  const root = document.documentElement;

  if (animate) {
    // La classe abilita la transizione dei colori solo durante il cambio,
    // così hover e interazioni normali restano istantanei.
    root.classList.add("theme-transition");
    clearTimeout(transitionTimer);
    transitionTimer = setTimeout(
      () => root.classList.remove("theme-transition"),
      TRANSITION_MS,
    );
  }

  if (theme === "auto") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", theme);

  // Allinea la barra del titolo mobile al fondo del tema attivo.
  const bg = getComputedStyle(root).getPropertyValue("--bg").trim();
  document.querySelector('meta[name="theme-color"]')?.setAttribute("content", bg || "#16181d");
}

/** Applica il tema salvato all'avvio, senza animazione. */
export function initTheme() {
  applyTheme(getTheme(), false);
}
