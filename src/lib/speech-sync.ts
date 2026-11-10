/** TTSが読む上限(UTF-8のバイト数)。src-tauri/src/services/tts.rs の MAX_INPUT_BYTES と揃える。 */
const TTS_MAX_BYTES = 4500;

/** 句読点などの後に入る間を、文字数に換算した重み(通常の1文字を1とする)。 */
const PAUSE: Record<string, number> = {
  "。": 3, "！": 3, "？": 3, "!": 3, "?": 3, "\n": 3,
  "、": 1.5, "，": 1.5, ",": 1.5, "．": 3, ".": 1.5, "：": 1, ":": 1, "；": 1.5, ";": 1.5,
};

const encoder = new TextEncoder();

/** 表の区切り行(`| --- | :-: |`)。 */
const TABLE_SEPARATOR = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/;

/**
 * 返答の各文字(UTF-16コード単位)を読み上げるか。Markdownの記号、コードブロック、表、リンク先、画像は読まない。
 * 返答は画面にMarkdownで描画され、読み上げには地の文だけを使う。
 */
export function spokenMask(text: string): boolean[] {
  const mask: boolean[] = new Array(text.length).fill(true);
  const hide = (from: number, to: number) => {
    for (let i = from; i < to; i++) mask[i] = false;
  };
  const lines: { start: number; end: number; body: string }[] = [];
  for (let pos = 0; pos <= text.length; ) {
    const nl = text.indexOf("\n", pos);
    const end = nl < 0 ? text.length : nl;
    lines.push({ start: pos, end: nl < 0 ? end : end + 1, body: text.slice(pos, end) });
    if (nl < 0) break;
    pos = nl + 1;
  }
  let inFence = false;
  let inTable = false;
  lines.forEach((line, idx) => {
    const trimmed = line.body.trim();
    if (/^(```|~~~)/.test(trimmed)) {
      inFence = !inFence;
      hide(line.start, line.end);
      return;
    }
    if (inFence) return hide(line.start, line.end);
    // 表: 区切り行の直前の見出し行から、`|`を含む行が続く間
    const next = lines[idx + 1]?.body ?? "";
    if (!inTable && trimmed.includes("|") && next.includes("|") && TABLE_SEPARATOR.test(next)) inTable = true;
    if (inTable) {
      if (trimmed.includes("|")) return hide(line.start, line.end);
      inTable = false;
    }
    if (/^\s*([-*_]\s*){3,}$/.test(line.body)) return hide(line.start, line.end);
    const prefix = /^(\s{0,3}#{1,6}\s+|\s*>+\s?|\s*([-*+]|\d+[.)])\s+)/.exec(line.body);
    if (prefix) hide(line.start, line.start + prefix[0].length);
    const mark = (re: RegExp, groups: (m: RegExpExecArray) => [number, number][]) => {
      for (const m of line.body.matchAll(re)) {
        for (const [a, b] of groups(m as RegExpExecArray)) hide(line.start + a, line.start + b);
      }
    };
    // 画像は全体、リンクは文字だけを残す
    mark(/!\[[^\]]*\]\([^)]*\)/g, (m) => [[m.index!, m.index! + m[0].length]]);
    mark(/(?<!!)\[([^\]]+)\]\(([^)]*)\)/g, (m) => [
      [m.index!, m.index! + 1],
      [m.index! + 1 + m[1].length, m.index! + m[0].length],
    ]);
    mark(/`+/g, (m) => [[m.index!, m.index! + m[0].length]]);
    mark(/\*\*|__|~~/g, (m) => [[m.index!, m.index! + 2]]);
    mark(/(?<!\*)\*(?=[^*\s])[^*\n]*[^*\s]\*(?!\*)/g, (m) => [
      [m.index!, m.index! + 1],
      [m.index! + m[0].length - 1, m.index! + m[0].length],
    ]);
  });
  return mask;
}

/** TTSへ送る本文(`spokenMask`で読む文字だけ)。 */
export function speechText(text: string): string {
  const mask = spokenMask(text);
  let out = "";
  for (let i = 0; i < text.length; i++) if (mask[i]) out += text[i];
  return out.trim();
}

/** 実際に読み上げられる先頭部分の、返答上の長さ(UTF-16コード単位)。読まない文字は数えず、上限を超える分はTTSに送られない。 */
export function spokenLength(text: string): number {
  const mask = spokenMask(text);
  let bytes = 0;
  let i = 0;
  while (i < text.length) {
    const cp = text.codePointAt(i)!;
    const width = cp > 0xffff ? 2 : 1;
    if (mask[i]) {
      const size = encoder.encode(String.fromCodePoint(cp)).length;
      if (bytes + size > TTS_MAX_BYTES) break;
      bytes += size;
    }
    i += width;
  }
  return i;
}

/**
 * 読み上げの進行度(0〜1)に対して、返答の先頭から何文字まで読み終えたとみなすか。
 * 読み上げ時間を読む文字数で按分し、句読点の間だけ重みを足してずれを抑える。読まない文字は重みを持たない。
 */
export function revealedLength(text: string, progress: number): number {
  const end = spokenLength(text);
  if (progress >= 1) return end;
  const mask = spokenMask(text);
  const weights: number[] = [];
  let total = 0;
  for (let i = 0; i < end; i++) {
    const w = mask[i] ? 1 + (PAUSE[text[i]] ?? 0) : 0;
    weights.push(w);
    total += w;
  }
  const target = Math.max(0, progress) * total;
  let acc = 0;
  for (let i = 0; i < end; i++) {
    acc += weights[i];
    if (acc > target) return i;
  }
  return end;
}
