/** TTSが読む上限(UTF-8のバイト数)。src-tauri/src/services/tts.rs の MAX_INPUT_BYTES と揃える。 */
const TTS_MAX_BYTES = 4500;

/** 句読点などの後に入る間を、文字数に換算した重み(通常の1文字を1とする)。 */
const PAUSE: Record<string, number> = {
  "。": 3, "！": 3, "？": 3, "!": 3, "?": 3, "\n": 3,
  "、": 1.5, "，": 1.5, ",": 1.5, "．": 3, ".": 1.5, "：": 1, ":": 1, "；": 1.5, ";": 1.5,
};

const encoder = new TextEncoder();

/** 実際に読み上げられる先頭部分の長さ(UTF-16コード単位)。上限を超える分はTTSに送られない。 */
export function spokenLength(text: string): number {
  let bytes = 0;
  let i = 0;
  while (i < text.length) {
    const cp = text.codePointAt(i)!;
    const size = encoder.encode(String.fromCodePoint(cp)).length;
    if (bytes + size > TTS_MAX_BYTES) break;
    bytes += size;
    i += cp > 0xffff ? 2 : 1;
  }
  return i;
}

/**
 * 読み上げの進行度(0〜1)に対して、先頭から何文字まで読み終えたとみなすか。
 * 読み上げ時間を文字数で按分し、句読点の間だけ重みを足してずれを抑える。
 */
export function revealedLength(text: string, progress: number): number {
  const end = spokenLength(text);
  if (progress >= 1) return end;
  const weights: number[] = [];
  let total = 0;
  for (let i = 0; i < end; i++) {
    const w = 1 + (PAUSE[text[i]] ?? 0);
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
