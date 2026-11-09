import { useEffect } from "react";

/** 入力欄(文字を打ち込む要素)にフォーカスがあるか。 */
function isTyping(target: EventTarget | null) {
  const el = target as HTMLElement | null;
  return !!el && (el.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(el.tagName));
}

/**
 * ファイル編集画面のキーボードショートカット。
 * - 閲覧中に `e`: 編集を始める(入力欄にフォーカスがある間や修飾キー併用時は無効)。
 * - Cmd/Ctrl+S: 保存する(入力欄にフォーカスがあっても有効)。
 */
export function useEditShortcuts(opts: { canEdit: boolean; editMode: boolean; onEdit: () => void; onSave: () => void }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const { canEdit, editMode, onEdit, onSave } = opts;
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        onSave();
      } else if (e.key === "e" && !e.metaKey && !e.ctrlKey && !e.altKey && !e.isComposing && canEdit && !editMode && !isTyping(e.target)) {
        e.preventDefault();
        onEdit();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
}
