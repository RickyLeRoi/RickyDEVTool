import { STORAGE_KEYS, type Theme } from "./constants";
import { DEFAULT_THEME } from "./defaults";
import {
  THEME_ATTRIBUTE,
  THEME_BG_CSS_VAR,
  THEME_COLOR_FALLBACK,
  THEME_TRANSITION_CLASS,
  THEME_TRANSITION_MS,
} from "./styles";

export type { Theme };

let transitionTimer: ReturnType<typeof setTimeout> | undefined;

export function getTheme(): Theme {
  const stored = localStorage.getItem(STORAGE_KEYS.theme);
  return stored === "light" || stored === "dark" ? stored : DEFAULT_THEME;
}

export function applyTheme(theme: Theme, animate = false) {
  localStorage.setItem(STORAGE_KEYS.theme, theme);
  const root = document.documentElement;

  if (animate) {
    root.classList.add(THEME_TRANSITION_CLASS);
    clearTimeout(transitionTimer);
    transitionTimer = setTimeout(
      () => root.classList.remove(THEME_TRANSITION_CLASS),
      THEME_TRANSITION_MS,
    );
  }

  if (theme === "auto") root.removeAttribute(THEME_ATTRIBUTE);
  else root.setAttribute(THEME_ATTRIBUTE, theme);

  const bg = getComputedStyle(root).getPropertyValue(THEME_BG_CSS_VAR).trim();
  document
    .querySelector('meta[name="theme-color"]')
    ?.setAttribute("content", bg || THEME_COLOR_FALLBACK);
}

export function initTheme() {
  applyTheme(getTheme(), false);
}
