export interface NotesLine {
  kind: "heading" | "item" | "text";
  text: string;
}

const PR_LINK = /\s+by\s+@\S+\s+in\s+https:\/\/github\.com\/[^\s/]+\/[^\s/]+\/pull\/(\d+)\s*$/;

// Turns the notes GitHub generates ("## What's Changed", "* title by @user in <PR url>",
// "**Full Changelog**: <url>") into short plain lines for the update dialog.
export function parseReleaseNotes(notes: string): NotesLine[] {
  const lines: NotesLine[] = [];
  for (const raw of notes.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || /^\*\*Full Changelog\*\*/i.test(line)) continue;
    const heading = /^#{1,6}\s+(.*)$/.exec(line);
    if (heading) {
      lines.push({ kind: "heading", text: heading[1] });
      continue;
    }
    const item = /^[*-]\s+(.*)$/.exec(line);
    if (item) {
      lines.push({ kind: "item", text: item[1].replace(PR_LINK, " (#$1)") });
      continue;
    }
    lines.push({ kind: "text", text: line.replace(/\*\*/g, "") });
  }
  return lines;
}
