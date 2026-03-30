import { Duration } from "./audio";

export function formatDuration(duration: Duration): string {
  const secsTotal = duration.secs + duration.nanos / 1e9;

  const hours = Math.floor(secsTotal / 3600);
  const minutes = Math.floor((secsTotal % 3600) / 60);
  const seconds = Math.floor(secsTotal % 60);

  if (hours > 0) {
    return `${hours}:${minutes.toString().padStart(2, "0")}:${seconds
      .toString()
      .padStart(2, "0")}`;
  }

  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
}
