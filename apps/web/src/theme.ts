// The page's colour theme: the system's (auto), light or dark. `style.css`
// reads `data-theme` on the root element; the choice is remembered in this
// browser only.

export type Theme = "auto" | "light" | "dark";

const THEMES: readonly Theme[] = ["auto", "light", "dark"];
const STORAGE_KEY = "anygamepad.theme";

/** The viewer's last choice; auto when none, or when storage is blocked. */
export const storedTheme = (): Theme => {
  try {
    const value = localStorage.getItem(STORAGE_KEY);
    return THEMES.find((theme) => theme === value) ?? "auto";
  } catch {
    return "auto";
  }
};

/** Applies `theme` to the page and, with `remember`, keeps it for next time. */
export const applyTheme = (theme: Theme, remember: boolean): void => {
  const root = document.documentElement;
  if (theme === "auto") delete root.dataset.theme;
  else root.dataset.theme = theme;
  if (!remember) return;
  try {
    if (theme === "auto") localStorage.removeItem(STORAGE_KEY);
    else localStorage.setItem(STORAGE_KEY, theme);
  } catch {
    // Private mode or blocked storage: the choice lasts until the page closes.
  }
};
