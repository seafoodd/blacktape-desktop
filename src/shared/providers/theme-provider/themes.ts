export interface ThemePalette {
  id: string;
  name: string;
  type: "light" | "dark";
  colors: {
    background: string;
    surface: string;
    text: string;
    muted: string;
    primary: string;
    secondary: string;
    border: string;
  };
}

export const builtinThemes: Record<string, ThemePalette> = {
  defaultDark: {
    id: "defaultDark",
    name: "Default Dark",
    type: "dark",
    colors: {
      background: "#0F1015",
      surface: "#161822",
      text: "#F1F2F6",
      muted: "#8E92A6",
      primary: "#005FCC",
      secondary: "#006DE4",
      border: "#1F2230",
    },
  },
  legacyDark: {
    id: "legacyDark",
    name: "Legacy Dark",
    type: "dark",
    colors: {
      background: "#1E1E20",
      surface: "#29292B",
      text: "#F5F5F5",
      muted: "#8F8F9C",
      primary: "#8C80F5",
      secondary: "#8BE9FD",
      border: "#444547",
    },
  },
  dracula: {
    id: "dracula",
    name: "Dracula",
    type: "dark",
    colors: {
      background: "#21222C",
      surface: "#282A36",
      text: "#F8F8F2",
      muted: "#6272A4",
      primary: "#BD93F9",
      secondary: "#A29BFB",
      border: "#6272A4",
    },
  },
  // Add Catppuccin, Nord, Light themes, etc.
};
