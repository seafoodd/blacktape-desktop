import { useEffect, useState } from "react";

export function useDominantColor(imageSrc: string | undefined): string {
  const [color, setColor] = useState("var(--color-surface)");

  useEffect(() => {
    if (!imageSrc) return;

    const img = new Image();
    img.crossOrigin = "Anonymous";
    img.src = imageSrc;

    img.onload = () => {
      const canvas = document.createElement("canvas");
      const ctx = canvas.getContext("2d");
      if (!ctx) return;

      canvas.width = 50;
      canvas.height = 50;
      ctx.drawImage(img, 0, 0, 50, 50);

      const { data } = ctx.getImageData(0, 0, 50, 50);

      let r = 0,
        g = 0,
        b = 0,
        count = 0;
      for (let i = 0; i < data.length; i += 16) {
        r += data[i];
        g += data[i + 1];
        b += data[i + 2];
        count++;
      }

      // Desaturate or darken extreme bright colors slightly for theme harmony
      const avgR = Math.round(r / count);
      const avgG = Math.round(g / count);
      const avgB = Math.round(b / count);

      setColor(`${avgR}, ${avgG}, ${avgB}`);
    };
  }, [imageSrc]);

  return color;
}
