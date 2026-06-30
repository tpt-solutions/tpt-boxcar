import { createContext, useContext, useState, useEffect, ReactNode } from "react";

type Theme = "light" | "dark";

interface ThemeContextValue {
  theme: Theme;
  toggle: () => void;
}

const ThemeContext = createContext<ThemeContextValue>({
  theme: "dark",
  toggle: () => {},
});

export function useTheme() {
  return useContext(ThemeContext);
}

const STORAGE_KEY = "origin-theme";

const lightVars: Record<string, string> = {
  "--bg-primary": "#ffffff",
  "--bg-secondary": "#f6f8fa",
  "--bg-nav": "#f0f2f5",
  "--text-primary": "#1f2328",
  "--text-secondary": "#656d76",
  "--border": "#d0d7de",
  "--accent": "#1a7f37",
  "--accent-hover": "#2da44e",
};

const darkVars: Record<string, string> = {
  "--bg-primary": "#0d1117",
  "--bg-secondary": "#161b22",
  "--bg-nav": "#0d1117",
  "--text-primary": "#e6edf3",
  "--text-secondary": "#8b949e",
  "--border": "#30363d",
  "--accent": "#1a7f37",
  "--accent-hover": "#2da44e",
};

function applyTheme(theme: Theme) {
  const vars = theme === "light" ? lightVars : darkVars;
  const root = document.documentElement;
  for (const [key, value] of Object.entries(vars)) {
    root.style.setProperty(key, value);
  }
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setTheme] = useState<Theme>(() => {
    const stored = localStorage.getItem(STORAGE_KEY);
    return stored === "light" || stored === "dark" ? stored : "dark";
  });

  useEffect(() => {
    applyTheme(theme);
    localStorage.setItem(STORAGE_KEY, theme);
  }, [theme]);

  const toggle = () => setTheme((t) => (t === "dark" ? "light" : "dark"));

  return (
    <ThemeContext.Provider value={{ theme, toggle }}>
      {children}
    </ThemeContext.Provider>
  );
}
