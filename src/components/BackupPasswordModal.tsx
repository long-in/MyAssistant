"use client";

import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useT } from "@/lib/i18n";

type Props = {
  /** `create`: 任意で設定(2回入力)。`restore`: 暗号化されたバックアップを開くために入力。 */
  mode: "create" | "restore";
  /** 復元で、直前のパスワードが違っていた。 */
  wrong?: boolean;
  /** 決定。作成で空なら null(暗号化しない)。 */
  onSubmit: (password: string | null) => void;
  onClose: () => void;
};

/** バックアップのパスワード入力。入力値はこの画面の状態にだけ置き、保存しない。 */
export default function BackupPasswordModal({ mode, wrong, onSubmit, onClose }: Props) {
  const t = useT();
  const [pw, setPw] = useState("");
  const [again, setAgain] = useState("");
  const ref = useRef<HTMLInputElement>(null);
  const create = mode === "create";
  const mismatch = create && pw !== again;
  const disabled = mismatch || (!create && pw === "");

  useEffect(() => {
    ref.current?.focus();
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const submit = () => {
    if (!disabled) onSubmit(pw === "" ? null : pw);
  };

  return createPortal(
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/40 p-6 backdrop-blur-sm">
      <form
        role="dialog"
        aria-modal="true"
        aria-labelledby="backup-pw-title"
        className="flex w-full max-w-md flex-col gap-3 rounded-xl border bg-card/80 p-5 text-sm shadow-2xl backdrop-blur-md"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <h2 id="backup-pw-title" className="text-base font-medium">
          {create ? t("バックアップのパスワード(任意)") : t("パスワードが必要です")}
        </h2>
        <p className="text-xs text-muted-foreground">
          {create
            ? t("入力すると、すべてのファイルをAES-256で暗号化します。空のままなら暗号化しません。パスワードを忘れると復元できません。")
            : t("このバックアップは暗号化されています。書き出したときのパスワードを入力してください。")}
        </p>
        {wrong && <p className="text-xs font-medium text-destructive">{t("パスワードが違います。")}</p>}
        <Input
          ref={ref}
          type="password"
          autoComplete="off"
          value={pw}
          onChange={(e) => setPw(e.target.value)}
          placeholder={t("パスワード")}
        />
        {create && (
          <Input
            type="password"
            autoComplete="off"
            value={again}
            onChange={(e) => setAgain(e.target.value)}
            placeholder={t("パスワード(確認)")}
          />
        )}
        {mismatch && <p className="text-xs text-destructive">{t("確認のパスワードが一致しません。")}</p>}
        <div className="mt-1 flex justify-end gap-2">
          <Button type="button" variant="outline" onClick={onClose}>
            {t("キャンセル")}
          </Button>
          <Button type="submit" disabled={disabled}>
            {create ? t("次へ") : t("開く")}
          </Button>
        </div>
      </form>
    </div>,
    document.body,
  );
}
