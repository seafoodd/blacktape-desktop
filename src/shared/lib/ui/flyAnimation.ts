export function flyToLibrary(fromRect: DOMRect, coverUrl?: string) {
  const targetEl = document.querySelector(
    '[data-tab-id="library"]',
  ) as HTMLElement | null;
  const targetRect = targetEl?.getBoundingClientRect() ?? {
    left: 40,
    top: 40,
    width: 80,
    height: 36,
  };

  const startX = fromRect.left + fromRect.width / 2;
  const startY = fromRect.top + fromRect.height / 2;
  const endX = targetRect.left + targetRect.width / 2;
  const endY = targetRect.top + targetRect.height / 2;

  const flyer = document.createElement("div");
  flyer.style.position = "fixed";
  flyer.style.left = `${startX - 18}px`;
  flyer.style.top = `${startY - 18}px`;
  flyer.style.width = "36px";
  flyer.style.height = "36px";
  flyer.style.borderRadius = "50%";
  flyer.style.pointerEvents = "none";
  flyer.style.zIndex = "999999";
  flyer.style.boxShadow =
    "0 8px 24px rgba(0, 0, 0, 0.5), 0 0 12px var(--color-primary, #1db954)";
  flyer.style.overflow = "hidden";
  flyer.style.display = "flex";
  flyer.style.alignItems = "center";
  flyer.style.justifyContent = "center";
  flyer.style.backgroundColor = "var(--color-primary, #1db954)";
  flyer.style.color = "#ffffff";
  flyer.style.border = "2px solid rgba(255, 255, 255, 0.8)";

  if (coverUrl) {
    const img = document.createElement("img");
    img.src = coverUrl;
    img.referrerPolicy = "no-referrer";
    img.style.width = "100%";
    img.style.height = "100%";
    img.style.objectFit = "cover";
    flyer.appendChild(img);
  } else {
    flyer.innerHTML = `<svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor"><path d="M12 3v10.55c-.59-.34-1.27-.55-2-.55-2.21 0-4 1.79-4 4s1.79 4 4 4 4-1.79 4-4V7h4V3h-6z"/></svg>`;
  }

  document.body.appendChild(flyer);

  const deltaX = endX - startX;
  const deltaY = endY - startY;

  const animation = flyer.animate(
    [
      {
        transform: "translate(0px, 0px) scale(1)",
        opacity: 1,
      },
      {
        transform: `translate(${deltaX * 0.4}px, ${deltaY * 0.25 - 40}px) scale(1.15)`,
        opacity: 0.95,
        offset: 0.35,
      },
      {
        transform: `translate(${deltaX}px, ${deltaY}px) scale(0.25)`,
        opacity: 0.2,
      },
    ],
    {
      duration: 550,
      easing: "cubic-bezier(0.22, 1, 0.36, 1)",
      fill: "forwards",
    },
  );

  animation.onfinish = () => {
    flyer.remove();
    if (targetEl) {
      targetEl.animate(
        [
          { transform: "scale(1)" },
          { transform: "scale(1.08)", offset: 0.5 },
          { transform: "scale(1)" },
        ],
        {
          duration: 300,
          easing: "ease-out",
        },
      );
    }
  };
}
