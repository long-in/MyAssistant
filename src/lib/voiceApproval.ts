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

/** 否定の形を含むが、意味は肯定の言い回し(「問題ない」「構いません」)。否定の判定より先に取り除く。 */
const YES_PHRASES = norm([
  "問題ない", "問題ありません", "問題なし", "構わない", "構いません", "かまわない", "かまいません", "間違いない", "間違いありません",
  "差し支えない", "差し支えありません", "異論ない", "異論ありません",
]);

/**
 * 否定の形。「許可しない」「実行しません」「大丈夫じゃない」のように、肯定の語を打ち消す言い方を拾う。
 * 単語表の否定語(`NO`)に無くても、ここに当たれば否定として扱う。
 */
const NEGATION = /ない|ません|なかった|せず|するな|やるな|無理|不可|困る|嫌|まだ|あとで|後で|don't|dont|not/;

/** 音声の「はい」で承認できる種類か。コマンドの個別承認・許可リストへの追加・Claude Code のツールの確認は、画面のボタンでだけ承認できる(拒否は音声でもできる)。 */
export function voiceCanApprove(kind: string): boolean {
  return kind !== "command" && kind !== "allow_command" && kind !== "tool_use";
}

/**
 * 承認要求への音声の返答を判定する。肯定とはっきり分かる場合だけ"yes"。
 * 否定の語・否定の形が1つでも含まれれば、肯定の語があっても"no"(「はい、でもやめて」「許可しない」は実行しない)。
 * どちらでもなければ"unclear"で、承認待ちを続ける。
 */
export function judgeAnswer(text: string): Answer {
  const t = normalize(text);
  if (!t || t.length > MAX_CHARS) return "unclear";
  // 「問題ない」などは肯定。否定の形として数えないよう、先に取り除く
  let rest = t;
  let affirmed = false;
  for (const p of YES_PHRASES) {
    if (rest.includes(p)) {
      affirmed = true;
      rest = rest.split(p).join("");
    }
  }
  if (NO_WORDS.some((w) => rest.includes(w)) || NEGATION.test(rest)) return "no";
  if (affirmed || YES_WORDS.some((w) => rest.includes(w))) return "yes";
  return "unclear";
}
