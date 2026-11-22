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
  selectedTab: string,
): boolean {
  if (selectedTab.trim().length > 0) return false;
  return tabsCount === 0 && albumsCount === 0;
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
