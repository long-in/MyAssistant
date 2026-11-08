/** 先頭のYAML Frontmatterを本文から分離し、タグだけ取り出す。 */
export function splitFrontmatter(text: string): { tags: string[]; body: string } {
  const m = text.match(/^---\n([\s\S]*?)\n---\n/);
  if (!m) return { tags: [], body: text };
  let tags: string[] = [];
  const line = m[1].split("\n").find((l) => l.startsWith("tags: "));
  try {
    if (line) tags = JSON.parse(line.slice(6));
  } catch {
    // 手編集で壊れていてもタグ表示を省くだけ
  }
  return { tags, body: text.slice(m[0].length) };
}

