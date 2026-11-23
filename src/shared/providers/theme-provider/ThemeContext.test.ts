import { describe, expect, it } from "vitest";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import {
  ThemeContext,
  ThemeContextValue,
  defaultThemeValue,
  useTheme,
} from "./ThemeContext.ts";
import { builtinThemes } from "./themes.ts";

describe("ThemeContext", () => {
  it("provides safe fallback defaults that do not throw", () => {
    expect(defaultThemeValue.activeThemeId).toBe("defaultDark");
    expect(defaultThemeValue.fontSize).toBe("14px");
    expect(defaultThemeValue.bgOpacity).toBe(1);
    expect(defaultThemeValue.bgBlur).toBe(false);
    expect(defaultThemeValue.palette).toEqual(builtinThemes.defaultDark);

    // Calling fallback setters should be safe no-ops
    expect(() => defaultThemeValue.setThemeId("matrix")).not.toThrow();
    expect(() => defaultThemeValue.setFontSize("16px")).not.toThrow();
    expect(() => defaultThemeValue.setBgOpacity(0.8)).not.toThrow();
    expect(() => defaultThemeValue.setBgBlur(true)).not.toThrow();
    expect(() => defaultThemeValue.setEnableDynamicArt(true)).not.toThrow();
  });

  it("returns default fallback without throwing when useTheme is called outside ThemeProvider", () => {
    let captured: ThemeContextValue | null = null;

    function Consumer() {
      captured = useTheme();
      return null;
    }

    renderToStaticMarkup(React.createElement(Consumer));

    expect(captured).not.toBeNull();
    expect(captured!.activeThemeId).toBe("defaultDark");
    expect(captured!.fontSize).toBe("14px");
    expect(captured!.palette).toEqual(builtinThemes.defaultDark);
  });

  it("returns context provider value when useTheme is called inside ThemeContext.Provider", () => {
    let captured: ThemeContextValue | null = null;

    function Consumer() {
      captured = useTheme();
      return null;
    }

    const customValue: ThemeContextValue = {
      ...defaultThemeValue,
      activeThemeId: "customTheme",
      fontSize: "18px",
    };

    renderToStaticMarkup(
      React.createElement(
        ThemeContext.Provider,
        { value: customValue },
        React.createElement(Consumer),
      ),
    );

    expect(captured).not.toBeNull();
    expect(captured!.activeThemeId).toBe("customTheme");
    expect(captured!.fontSize).toBe("18px");
  });
});
