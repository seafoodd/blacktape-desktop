/** Small floating label shown under the cursor while dragging (e.g. an album). */
export function setDragGhost(e: React.DragEvent, title: string, subtitle: string) {
  const ghost = document.createElement("div");
  ghost.style.cssText =
    "position:absolute;top:-9999px;left:-9999px;display:flex;flex-direction:column;gap:2px;" +
    "padding:8px 12px;width:220px;border-radius:8px;background:rgba(18,18,18,0.95);" +
    "border:1px solid rgba(255,255,255,0.12);box-shadow:0 12px 32px rgba(0,0,0,0.6);" +
    "color:#fff;font-family:inherit;pointer-events:none;";

  const titleEl = document.createElement("span");
  titleEl.textContent = title;
  titleEl.style.cssText =
    "font-size:13px;font-weight:600;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;";
  const subtitleEl = document.createElement("span");
  subtitleEl.textContent = subtitle;
  subtitleEl.style.cssText =
    "font-size:11px;font-weight:500;color:var(--color-primary, #1db954);";
  ghost.append(titleEl, subtitleEl);

  document.body.appendChild(ghost);
  e.dataTransfer.setDragImage(ghost, 20, 20);
  requestAnimationFrame(() => document.body.removeChild(ghost));
}
