/** Decodes HTML entities (e.g. `&amp;`, `&#39;`) into plain text. */
export function decodeHtmlEntities(text: string): string {
  if (!text || !text.includes("&")) return text;
  const doc = new DOMParser().parseFromString(text, "text/html");
  return doc.documentElement.textContent ?? text;
}
