import {
  createContext,
  ReactNode,
  useContext,
  useEffect,
  useState,
} from "react";
import { builtinThemes, ThemePalette } from "./themes";
// tauri fs module for custom CSS
// import { readTextFile, BaseDirectory } from '@tauri-apps/api/fs';

interface ThemeContextValue {
  activeThemeId: string;
  setThemeId: (id: string) => void;
  palette: ThemePalette;
  enableDynamicArt: boolean;
  setEnableDynamicArt: (val: boolean) => void;
}

const ThemeContext = createContext<ThemeContextValue | undefined>(undefined);

export const ThemeProvider = ({ children }: { children: ReactNode }) => {
  const [activeThemeId, setThemeId] = useState<string>(() => {
    return localStorage.getItem("theme_id") || "defaultDark";
  });

  const [enableDynamicArt, setEnableDynamicArt] = useState<boolean>(() => {
    return localStorage.getItem("theme_dynamic_art") === "true";
  });

  const palette = builtinThemes[activeThemeId] || builtinThemes.defaultDark;

  useEffect(() => {
    const root = document.documentElement;

    // Inject all palette colors as CSS variables
    Object.entries(palette.colors).forEach(([key, value]) => {
      root.style.setProperty(`--color-${key}`, value);
    });

    // Tag the root with the theme type so global CSS can adjust shadows/inversions if needed
    root.setAttribute("data-theme-type", palette.type);
    localStorage.setItem("theme_id", activeThemeId);
  }, [activeThemeId, palette]);

  // Example: Load Custom CSS from Tauri Filesystem
  /*
  useEffect(() => {
    async function loadCustomCss() {
      try {
        const css = await readTextFile('custom.css', { dir: BaseDirectory.AppData });
        const styleEl = document.createElement('style');
        styleEl.id = 'blacktape-custom-css';
        styleEl.innerHTML = css;
        document.head.appendChild(styleEl);
      } catch (e) {
        // No custom css found
      }
    }
    loadCustomCss();
  }, []);
  */

  return (
    <ThemeContext.Provider
      value={{
        activeThemeId,
        setThemeId,
        palette,
        enableDynamicArt,
        setEnableDynamicArt,
      }}
    >
      {children}
    </ThemeContext.Provider>
  );
};

export const useTheme = () => {
  const context = useContext(ThemeContext);
  if (!context) throw new Error("useTheme must be used within a ThemeProvider");
  return context;
};
