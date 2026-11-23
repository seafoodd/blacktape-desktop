import { createContext, useContext } from "react";
import { builtinThemes, ThemePalette } from "./themes.ts";
import {
  DEFAULT_BG_BLUR,
  DEFAULT_BG_OPACITY,
} from "@/shared/providers/theme-provider/appearance.ts";

export interface ThemeContextValue {
  activeThemeId: string;
  setThemeId: (id: string) => void;
  palette: ThemePalette;
  enableDynamicArt: boolean;
  setEnableDynamicArt: (val: boolean) => void;
  fontSize: string;
  setFontSize: (size: string) => void;
  bgOpacity: number;
  setBgOpacity: (opacity: number) => void;
  bgBlur: boolean;
  setBgBlur: (blur: boolean) => void;
}

export const defaultThemeValue: ThemeContextValue = {
  activeThemeId: "defaultDark",
  setThemeId: () => {},
  palette: builtinThemes.defaultDark,
  enableDynamicArt: false,
  setEnableDynamicArt: () => {},
  fontSize: "14px",
  setFontSize: () => {},
  bgOpacity: DEFAULT_BG_OPACITY,
  setBgOpacity: () => {},
  bgBlur: DEFAULT_BG_BLUR,
  setBgBlur: () => {},
};

export const ThemeContext = createContext<ThemeContextValue>(defaultThemeValue);

export const useTheme = (): ThemeContextValue => {
  const context = useContext(ThemeContext);
  return context ?? defaultThemeValue;
};
