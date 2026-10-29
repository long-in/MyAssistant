import { normalize } from "./voiceApproval";

/** 会話を切り替えたいと受け取れる表現。表記は`normalize`で揃えて部分一致で調べる。 */
const SWITCH_PHRASES = [
  "新しい会話", "新しい話題", "会話をリセット", "会話リセット", "会話をクリア", "会話をきりかえ", "会話を切り替え", "会話を切りかえ",
  "会話を変え", "話題を変え", "話題をかえ", "話題を切り替え", "話は変わ", "話が変わ", "話かわ", "別の話", "別の話題",
  "最初からやり直", "はじめからやり直", "初めからやり直", "全部忘れ", "全て忘れ", "すべて忘れ",
];

/** 発話全体がこれだけのときも、切り替えの指示として扱う。 */
const SWITCH_ONLY = ["リセット", "リセットして", "リセットお願い", "リセットしてください"].map(normalize);

/** 切り替えの語を除いたあとに残る、文意に影響しない語尾。 */
const FILLER = /(してください|してくれ|してほしい|にして|にしたい|して|ください|お願いします|お願い|します|ます|です|たい|ほしい|けど|から|ね|よ|で|に|を|は|が|も|の)/g;

/** 残りがこの長さ以下なら、切り替えの指示だけとみなす。 */
const COMMAND_REST_MAX = 3;

export type SwitchIntent = {
  /** `command`: 切り替えの指示だけ。`mixed`: 切り替えの表現に続けて別の依頼が含まれる。 */
  kind: "command" | "mixed";
};

/** 発話に会話を切り替える表現があれば返す。切り替えるかは、確認してから決める。 */
export function detectSwitch(text: string): SwitchIntent | null {
  const t = normalize(text);
  if (!t) return null;
  if (SWITCH_ONLY.includes(t)) return { kind: "command" };
  const phrase = SWITCH_PHRASES.map(normalize).find((p) => t.includes(p));
  if (!phrase) return null;
  const rest = t.replace(phrase, "").replace(FILLER, "");
  return { kind: rest.length <= COMMAND_REST_MAX ? "command" : "mixed" };
}
