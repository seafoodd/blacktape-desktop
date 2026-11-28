export const INSPIRATION_TAGS = [
  "Electronic",
  "Ambient",
  "Lofi",
  "Radiohead",
  "Aphex Twin",
  "Daft Punk",
] as const;

export function cleanSearchQuery(query: string): string {
  return query.trim();
}

export function shouldShowEmptyHub(
  tabsCount: number,
  albumsCount: number,
  _selectedTab?: string,
  hasActiveDownloads = false,
): boolean {
  // If there are no artists and no albums, the library is genuinely empty,
  // unless a download is currently running.
  return tabsCount === 0 && albumsCount === 0 && !hasActiveDownloads;
}

export function formatFolderScanningLabel(
  isScanning: boolean,
  scannedCount?: number,
): string {
  if (!isScanning) return "Choose Music Folder";
  if (scannedCount !== undefined && scannedCount > 0) {
    return `Scanning folder (${scannedCount} songs)…`;
  }
  return "Scanning folder…";
}
