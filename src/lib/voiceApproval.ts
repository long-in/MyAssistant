/** 承認要求への音声の返答の判定。 */
export type Answer = "yes" | "no" | "unclear";

/** これより長い発話は、承認への返答ではなく別の話として扱う。 */
const MAX_CHARS = 30;

const YES = [
  "はい", "ハイ", "お願い", "おねがい", "オーケー", "オッケー", "ok", "okay", "yes", "yeah", "いいよ", "いいですよ",
  "実行して", "実行", "進めて", "すすめて", "やって", "大丈夫", "だいじょうぶ", "了解", "承認", "許可", "どうぞ", "goahead", "うん", "ええ", "そうして", "お願いします",
];

const NO = [
  "いいえ", "イイエ", "いえ", "いや", "ちがう", "違う", "違います", "だめ", "ダメ", "駄目", "やめ", "止め", "とめて", "中止", "キャンセル",
  "待って", "まって", "ストップ", "しないで", "やらないで", "結構", "拒否", "ノー", "no", "stop", "cancel", "却下",
];

/** 表記ゆれを揃える(全角半角・大文字小文字・空白と句読点の除去)。 */
export function normalize(text: string): string {
  return text
    .normalize("NFKC")
    .toLowerCase()
    .replace(/[\s、。,.!?！？「」『』…~〜ー-]/g, "")
    // 長音符は上で除去するため、判定語側も同じ規則で揃える
    .trim();
}

const norm = (words: string[]) => words.map(normalize).filter(Boolean);
const YES_WORDS = norm(YES);
const NO_WORDS = norm(NO);

/**
 * 承認要求への音声の返答を判定する。肯定とはっきり分かる場合だけ"yes"。
 * 否定の語が1つでも含まれれば、肯定の語があっても"no"(「はい、でもやめて」は実行しない)。
 * どちらでもなければ"unclear"で、承認待ちを続ける。
 */
export function judgeAnswer(text: string): Answer {
  const t = normalize(text);
  if (!t || t.length > MAX_CHARS) return "unclear";
  if (NO_WORDS.some((w) => t.includes(w))) return "no";
  if (YES_WORDS.some((w) => t.includes(w))) return "yes";
  return "unclear";
}
