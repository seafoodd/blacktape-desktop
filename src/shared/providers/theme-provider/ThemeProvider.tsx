import {
  createContext,
  ReactNode,
  useContext,
  useEffect,
  useState,
} from "react";
import { builtinThemes, ThemePalette } from "./themes";

interface ThemeContextValue {
  activeThemeId: string;
  setThemeId: (id: string) => void;
  palette: ThemePalette;
  enableDynamicArt: boolean;
  setEnableDynamicArt: (val: boolean) => void;
  fontSize: string;
  setFontSize: (size: string) => void;
}

const ThemeContext = createContext<ThemeContextValue | undefined>(undefined);

const themeChannel = new BroadcastChannel("blacktape_theme_sync");

export const ThemeProvider = ({ children }: { children: ReactNode }) => {
  const [activeThemeId, setActiveThemeId] = useState<string>(() => {
    return localStorage.getItem("theme_id") || "defaultDark";
  });

  const [enableDynamicArt, setEnableDynamicArtState] = useState<boolean>(() => {
    return localStorage.getItem("theme_dynamic_art") === "true";
  });

  const [fontSize, setFontSizeState] = useState<string>(() => {
    return localStorage.getItem("font_size") || "13px";
  });

  const palette = builtinThemes[activeThemeId] || builtinThemes.defaultDark;

  const setThemeId = (id: string) => {
    setActiveThemeId(id);
    localStorage.setItem("theme_id", id);
    themeChannel.postMessage({ type: "THEME_CHANGED", activeThemeId: id });
  };

  const setEnableDynamicArt = (val: boolean) => {
    setEnableDynamicArtState(val);
    localStorage.setItem("theme_dynamic_art", String(val));
    themeChannel.postMessage({
      type: "DYNAMIC_ART_CHANGED",
      enableDynamicArt: val,
    });
  };

  const setFontSize = (size: string) => {
    setFontSizeState(size);
    localStorage.setItem("font_size", size);
    themeChannel.postMessage({ type: "FONT_SIZE_CHANGED", fontSize: size });
  };

  useEffect(() => {
    const handleMessage = (event: MessageEvent) => {
      if (event.data.type === "THEME_CHANGED") {
        setActiveThemeId(event.data.activeThemeId);
        localStorage.setItem("theme_id", event.data.activeThemeId);
      }
      if (event.data.type === "DYNAMIC_ART_CHANGED") {
        setEnableDynamicArtState(event.data.enableDynamicArt);
        localStorage.setItem(
          "theme_dynamic_art",
          String(event.data.enableDynamicArt),
        );
      }
      if (event.data.type === "FONT_SIZE_CHANGED") {
        setFontSizeState(event.data.fontSize);
        localStorage.setItem("font_size", event.data.fontSize);
      }
    };

    themeChannel.addEventListener("message", handleMessage);
    return () => themeChannel.removeEventListener("message", handleMessage);
  }, []);

  useEffect(() => {
    const root = document.documentElement;
    Object.entries(palette.colors).forEach(([key, value]) => {
      root.style.setProperty(`--color-${key}`, value);
    });
    root.setAttribute("data-theme-type", palette.type);
  }, [palette]);

  useEffect(() => {
    document.documentElement.style.setProperty("--font-base", fontSize);
  }, [fontSize]);

  return (
    <ThemeContext.Provider
      value={{
        activeThemeId,
        setThemeId,
        palette,
        enableDynamicArt,
        setEnableDynamicArt,
        fontSize,
        setFontSize,
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
