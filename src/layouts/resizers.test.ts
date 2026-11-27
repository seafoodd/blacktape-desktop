import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const leftCss = readFileSync(fileURLToPath(new URL("./left-sidebar.module.css", import.meta.url)), "utf8");
const rightCss = readFileSync(fileURLToPath(new URL("./right-sidebar.module.css", import.meta.url)), "utf8");

function readRule(css: string, className: string): string {
  const match = new RegExp(`(?:^|[\\s,}])\\.${className}\\s*\\{([^}]*)\\}`).exec(css);
  if (!match) {
    throw new Error(`CSS rule ".${className}" not found`);
  }
  return match[1];
}

function readPx(rule: string, className: string, property: string): number {
  const match = new RegExp(`(?:^|[\\s;{])${property}\\s*:\\s*(-?\\d+(?:\\.\\d+)?)px`).exec(rule);
  if (!match) {
    throw new Error(`".${className}" has no ${property} in px`);
  }
  return Number(match[1]);
}

function rightSidebarResizer() {
  const sidebar = readRule(rightCss, "rightSidebar");
  const resizer = readRule(rightCss, "resizer");
  return {
    borderWidth: readPx(sidebar, "rightSidebar", "border-left"),
    left: readPx(resizer, "resizer", "left"),
    width: readPx(resizer, "resizer", "width"),
  };
}

function leftSidebarResizer() {
  const sidebar = readRule(leftCss, "leftSidebar");
  const resizer = readRule(leftCss, "resizer");
  return {
    borderWidth: readPx(sidebar, "leftSidebar", "border-right"),
    right: readPx(resizer, "resizer", "right"),
    width: readPx(resizer, "resizer", "width"),
  };
}

describe("sidebar resizers", () => {
  it("right sidebar resizer does not cover the main panel scrollbar", () => {
    const { borderWidth, left } = rightSidebarResizer();
    expect(left).toBeGreaterThanOrEqual(-borderWidth);
  });

  it("right sidebar resizer covers its whole border", () => {
    const { borderWidth, left, width } = rightSidebarResizer();
    expect(left).toBeLessThanOrEqual(-borderWidth);
    expect(left + width).toBeGreaterThanOrEqual(0);
  });

  it("left sidebar resizer does not cover its own scrollbar", () => {
    const { right, width } = leftSidebarResizer();
    expect(-right - width).toBeGreaterThanOrEqual(0);
  });

  it("left sidebar resizer covers its whole border", () => {
    const { borderWidth, right, width } = leftSidebarResizer();
    expect(-right - width).toBeLessThanOrEqual(0);
    expect(-right).toBeGreaterThanOrEqual(borderWidth);
  });
});
