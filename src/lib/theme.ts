export type Theme = "auto" | "light" | "dark";

const KEY = "rdt-theme";
const TRANSITION_MS = 1000;
let transitionTimer: ReturnType<typeof setTimeout> | undefined;

export function getTheme(): Theme {
  const stored = localStorage.getItem(KEY);
  return stored === "light" || stored === "dark" ? stored : "auto";
}

export function applyTheme(theme: Theme, animate = false) {
  localStorage.setItem(KEY, theme);
  const root = document.documentElement;

  if (animate) {
    root.classList.add("theme-transition");
    clearTimeout(transitionTimer);
    transitionTimer = setTimeout(
      () => root.classList.remove("theme-transition"),
      TRANSITION_MS,
    );
  }

  if (theme === "auto") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", theme);

  const bg = getComputedStyle(root).getPropertyValue("--bg").trim();
  document.querySelector('meta[name="theme-color"]')?.setAttribute("content", bg || "#16181d");
}

export function initTheme() {
  applyTheme(getTheme(), false);
}
