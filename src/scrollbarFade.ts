/**
 * Fades the scrollbar of the area under the mouse in and out. CSS cannot do this: Chromium does not repaint a
 * styled scrollbar while a transition runs. Setting the colour on the element for every frame does repaint it
 * (see the scrollbar rules in globals.css).
 */
export const SCROLLBAR_FADE_MS = 250;

/** Opacity of a bar under the mouse, as a share of the text colour. */
const SHOWN_PERCENT = 30;

interface Fade {
  /** 0 is hidden, 1 is fully shown. */
  level: number;
  from: number;
  to: 0 | 1;
  start: number;
}

export interface FadeClock {
  now: () => number;
  frame: (step: () => void) => void;
}

/** Moves every target towards "shown" while it is wanted and towards "hidden" otherwise. */
export function createFader<T>(apply: (target: T, level: number) => void, clock: FadeClock) {
  const fades = new Map<T, Fade>();
  let running = false;

  function step() {
    let moving = false;
    const now = clock.now();
    for (const [target, fade] of fades) {
      if (fade.level === fade.to) continue;
      const done = Math.min((now - fade.start) / SCROLLBAR_FADE_MS, 1);
      fade.level = done === 1 ? fade.to : fade.from + (fade.to - fade.from) * done;
      apply(target, fade.level);
      if (fade.level !== fade.to) moving = true;
      else if (fade.to === 0) fades.delete(target);
    }
    running = moving;
    if (moving) clock.frame(step);
  }

  return {
    show(wanted: ReadonlySet<T>) {
      const now = clock.now();
      for (const target of wanted) {
        if (!fades.has(target)) fades.set(target, { level: 0, from: 0, to: 0, start: now });
      }
      let changed = false;
      for (const [target, fade] of fades) {
        const to = wanted.has(target) ? 1 : 0;
        if (fade.to === to) continue;
        // A fade turned around midway continues from where it is.
        Object.assign(fade, { from: fade.level, to, start: now });
        changed = true;
      }
      if (changed && !running) {
        running = true;
        clock.frame(step);
      }
    },
  };
}

/** The scrolling areas that contain `element`, itself included. */
function scrollAreasAround(element: Element | null): Set<HTMLElement> {
  const areas = new Set<HTMLElement>();
  for (let node = element; node; node = node.parentElement) {
    if (!(node instanceof HTMLElement)) continue;
    const { overflowX, overflowY } = getComputedStyle(node);
    if (node instanceof HTMLTextAreaElement || /auto|scroll/.test(`${overflowX} ${overflowY}`)) {
      areas.add(node);
    }
  }
  return areas;
}

export function setupScrollbarFade(): void {
  const fader = createFader<HTMLElement>(
    (area, level) => {
      if (level === 0) area.style.removeProperty("--scrollbar-thumb");
      else {
        area.style.setProperty(
          "--scrollbar-thumb",
          `color-mix(in srgb, var(--color-text) ${(level * SHOWN_PERCENT).toFixed(1)}%, transparent)`,
        );
      }
    },
    { now: () => performance.now(), frame: (step) => requestAnimationFrame(step) },
  );
  document.addEventListener("pointerover", (event) => {
    fader.show(scrollAreasAround(event.target as Element | null));
  });
  // The pointer left the window.
  document.documentElement.addEventListener("pointerleave", () => fader.show(new Set()));
}
