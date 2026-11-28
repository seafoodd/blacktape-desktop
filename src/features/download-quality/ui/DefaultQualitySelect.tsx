import type { DownloadQualityPreset } from "@/shared/lib/audio.ts";

export function DefaultQualitySelect({ id, value, onChange, className }: {
  id: string;
  value: DownloadQualityPreset;
  onChange: (value: DownloadQualityPreset) => void;
  className?: string;
}) {
  return (
    <select id={id} className={className} value={value}
      onChange={(event) => onChange(event.target.value as DownloadQualityPreset)}>
      <option value="best">Best available · Lossless when found</option>
      <option value="high">High · 160–320 kbps</option>
      <option value="standard">Standard · About 128 kbps</option>
    </select>
  );
}

export function DefaultQualityDescription({ className }: { className?: string }) {
  return (
    <span className={className}>
      Best retains full fidelity when FLAC or another lossless source is available (roughly 20–40 MB for four minutes).
      High uses lossy compression (about 5–10 MB). Standard uses more compression (about 4 MB).
    </span>
  );
}
