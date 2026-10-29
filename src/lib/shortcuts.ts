export type ShortcutId = "mic" | "chat" | "knowledge" | "settings" | "help" | "abort";

export type Shortcut = {
  id: ShortcutId;
  /** "Mod"はmacOSでは⌘、それ以外ではCtrlとして表示する */
  keys: string[];
  label: string;
  description: string;
  scope: "どの画面でも" | "対話画面";
};

export const SHORTCUTS: Shortcut[] = [
  { id: "mic", keys: ["Space"], label: "押している間だけ録音", description: "押している間録音し、離してから約1秒後に録音を止めて文字起こし・送信します(語尾が切れないよう少し録り続けます。その間に押し直すと録音を続けます。ボタンのクリックは従来どおり開始 / 停止の切り替え)。入力欄にフォーカスがあるときは、通常のスペース入力になります。", scope: "対話画面" },
  { id: "abort", keys: ["Esc"], label: "強制中断", description: "進行中の処理(依頼の実行・読み上げ・録音・文字起こしの結果・確認待ち)をすべて止めます。周囲の雑音や話し声で意図せず処理が進んだときに使います。会話の文脈は残ります。", scope: "対話画面" },
  { id: "chat", keys: ["Mod", "1"], label: "対話を開く", description: "対話画面へ移動します。", scope: "どの画面でも" },
  { id: "knowledge", keys: ["Mod", "2"], label: "ナレッジを開く", description: "ナレッジ画面へ移動します。", scope: "どの画面でも" },
  { id: "settings", keys: ["Mod", "3"], label: "設定を開く", description: "APIキーなどの設定画面へ移動します。", scope: "どの画面でも" },
  { id: "help", keys: ["Mod", "/"], label: "ショートカット一覧を開く", description: "このページを開きます。", scope: "どの画面でも" },
];

/** 修飾キーなしのスペースで、文字入力の邪魔にならない場合のみtrue。 */
export function isPlainSpace(e: KeyboardEvent): boolean {
  if (e.code !== "Space" || e.metaKey || e.ctrlKey || e.altKey || e.shiftKey || e.isComposing) return false;
  const t = e.target;
  return !(t instanceof HTMLElement && (t.isContentEditable || t.closest("input, textarea, select, [role=textbox], [role=combobox]")));
}

/** キー入力をショートカットIDへ変換する。該当しなければnull。 */
export function matchShortcut(e: KeyboardEvent): ShortcutId | null {
  if (e.code === "Space") return isPlainSpace(e) ? "mic" : null;
  if (e.code === "Escape") return e.metaKey || e.ctrlKey || e.altKey || e.shiftKey ? null : "abort";
  if (!(e.metaKey || e.ctrlKey) || e.altKey) return null;
  if (e.shiftKey) return e.code === "KeyM" ? "mic" : null;
  switch (e.code) {
    case "Digit1": return "chat";
    case "Digit2": return "knowledge";
    case "Digit3": return "settings";
    case "Slash": return "help";
    default: return null;
  }
}

export function isMac(): boolean {
  return typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
}

export function keyLabel(key: string, mac: boolean): string {
  if (key === "Mod") return mac ? "⌘" : "Ctrl";
  if (key === "Shift") return mac ? "⇧" : "Shift";
  return key;
}
