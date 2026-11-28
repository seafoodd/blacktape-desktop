/** Torrent connection messages remain useful even after other tracks have made progress. */
export function torrentStatusMessage(message: string): string | null {
  return /^(Waiting for torrent|Connecting to torrent|Retrying torrent|Downloading from Archive web seed)/i.test(message)
    ? message
    : null;
}
