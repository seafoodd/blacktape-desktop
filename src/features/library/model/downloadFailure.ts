/** Turns the error of a failed song download into something the user can act on. Pure. */

export type FailureKind = "no-stream" | "restricted" | "network" | "other";

export interface FailureExplanation {
  kind: FailureKind;
  /** What happened, in plain words. */
  reason: string;
  /** What the user can do about it. */
  hint: string;
}

/** The failure of a song that the platform lists but gives no audio for (it can never be downloaded from there). */
export function isNoStreamFailure(message: string): boolean {
  return /no video formats found|no output file|produced no audio file/i.test(message);
}

/** Classifies a yt-dlp / download error message. `platform` is the display name of the song's platform. */
export function explainFailure(message: string, platform: string): FailureExplanation {
  const text = message.replace(/^download failed:\s*/i, "").trim();
  if (isNoStreamFailure(text)) {
    return {
      kind: "no-stream",
      reason: `This song has no audio stream on ${platform}.`,
      hint: "Pick another version of the album, or download just this song from YouTube or SoundCloud.",
    };
  }
  if (/age-restricted|sign in to confirm|requested format is not available|private video|members-only|login required/i.test(text)) {
    return {
      kind: "restricted",
      reason: `${platform} does not give out the audio of this song (age-restricted, private or blocked).`,
      hint: "Try another version of the album or another source for this song.",
    };
  }
  if (/http error 4(03|29)|timed out|timeout|unreachable|connection|network|name or service|temporary failure|resolve/i.test(text)) {
    return {
      kind: "network",
      reason: "The download failed because of a network or rate limit problem.",
      hint: "Check the connection and try again in a moment.",
    };
  }
  return {
    kind: "other",
    reason: text.length > 0 ? text.slice(0, 240) : "The download failed.",
    hint: "Try again, or pick another version or source for this song.",
  };
}
