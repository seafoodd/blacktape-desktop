/**
 * Whether the lyrics should scroll to the active line. While paused the reader may scroll freely: the view
 * only follows when the line itself changes, for example after a seek.
 */
export function shouldFollowLine({
  isPlaying,
  userScrolled,
  lineChanged,
}: {
  isPlaying: boolean;
  userScrolled: boolean;
  lineChanged: boolean;
}): boolean {
  return !userScrolled && (isPlaying || lineChanged);
}
