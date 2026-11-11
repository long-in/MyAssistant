"use client";

import { useEffect, useRef } from "react";
import { useT } from "@/lib/i18n";
import { voiceCanApprove } from "@/lib/voiceApproval";
import { Check, ListPlus, Loader2, Mic, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { TaskEvent } from "@/lib/api";

export type Approval = Extract<TaskEvent, { type: "approval" }>;

/** 音声での返答の待ち受け状態。 */
export type Listening = "preparing" | "listening" | "transcribing" | "idle";

const TITLES: Record<Approval["kind"], string> = {
  plan: "この手順で作業を進めてよいですか？",
  command: "このコマンドを実行してよいですか？",
  allow_command: "このコマンドは許可リストにありません。実行してよいですか？",
  file: "ファイルの作成を許可しますか？",
  skill_update: "スキルを次の内容で上書きしてよいですか？",
  skill_delete: "このスキルを削除してよいですか？",
  tool_use: "このツールを実行してよいですか？",
};

const LISTENING_LABEL: Record<Listening, string> = {
  preparing: "マイクを準備しています…",
  listening: "お話しください。「はい」で実行、「いいえ」で中止します",
  transcribing: "聞き取っています…",
  idle: "「はい」と話すか、ボタンで答えてください",
};

/** 音声では承認できない種類(コマンドの個別承認・許可リストへの追加)の待ち受け表示。拒否は音声でもできる。 */
const BUTTON_ONLY_LABEL: Record<Listening, string> = {
  preparing: "マイクを準備しています…",
  listening: "このコマンドの承認はボタンで行います(「いいえ」と話すと中止します)",
  transcribing: "聞き取っています…",
  idle: "このコマンドの承認はボタンで行います(「いいえ」と話すと中止します)",
};

/**
 * 実行前の確認を画面中央に重ねて表示する。背景はうっすら透けさせる。
 * 計画・ファイル作成・スキルの変更は、音声で肯定されたときも実行に進む(判定と録音は呼び出し側)。
 * コマンドの個別承認と許可リストへの追加は、周囲の音声で誤って通らないよう、ボタンでだけ承認できる。
 * 許可リストにないコマンドは、「今回だけ実行」(許可リストへ加えない)と「許可に加えて実行」を選べる。
 * 開いている間は、Tabでの移動をダイアログの中に留める(Escは対話画面の強制中断で、依頼ごと止まる)。
 */
export default function ApprovalModal({
  approval,
  listening,
  onAnswer,
}: {
  approval: Approval;
  listening: Listening;
  /** `once`: 許可リストにないコマンドを、許可へ加えずに今回だけ実行する。 */
  onAnswer: (approved: boolean, once?: boolean) => void;
}) {
  const t = useT();
  const dialog = useRef<HTMLDivElement>(null);
  const byVoice = voiceCanApprove(approval.kind);
  // 開いたら、ダイアログ自体へフォーカスを移す(ボタンには置かない。押しっぱなしのキーで、意図せず承認されないようにする)
  useEffect(() => {
    const before = document.activeElement as HTMLElement | null;
    dialog.current?.focus();
    return () => before?.focus?.();
  }, [approval.id]);
  /** Tabでの移動を、ダイアログの中のボタンで循環させる。 */
  const trap = (e: React.KeyboardEvent) => {
    if (e.key !== "Tab") return;
    const items = dialog.current?.querySelectorAll<HTMLElement>("button:not(:disabled)");
    if (!items || items.length === 0) return;
    const first = items[0];
    const last = items[items.length - 1];
    const active = document.activeElement;
    if (e.shiftKey && (active === first || active === dialog.current)) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && active === last) {
      e.preventDefault();
      first.focus();
    }
  };
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/40 p-6 backdrop-blur-sm">
      <div
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby="approval-title"
        tabIndex={-1}
        onKeyDown={trap}
        className="max-h-full outline-none w-full max-w-xl overflow-auto rounded-xl border border-primary/40 bg-card/70 p-5 text-sm shadow-2xl backdrop-blur-md"
      >
        <h2 id="approval-title" className="text-base font-medium">
          {t(TITLES[approval.kind])}
        </h2>

        {approval.kind === "plan" && (
          <>
            {approval.preview && <p className="mt-2 text-muted-foreground">{approval.preview}</p>}
            <ol className="mt-3 space-y-1.5">
              {approval.steps.map((s, i) => (
                <li key={i} className="flex gap-2">
                  <span className="w-5 shrink-0 text-right tabular-nums text-primary">{i + 1}.</span>
                  <span className="min-w-0 break-words">{s}</span>
                </li>
              ))}
            </ol>
          </>
        )}

        {(approval.kind === "command" || approval.kind === "allow_command") && (
          <>
            {approval.reason && <p className="mt-2 text-muted-foreground">{approval.reason}</p>}
            <p className="mt-2 text-xs text-muted-foreground">
              {t("実行場所:")} <span className="font-mono break-all">{approval.path}</span>
            </p>
            <pre className="mt-2 max-h-48 overflow-auto rounded-md border bg-background/60 p-2 font-mono text-xs whitespace-pre-wrap break-all">
              {approval.preview}
            </pre>
            {approval.kind === "allow_command" && (
              <p className="mt-2 text-xs text-muted-foreground">
                {t("「許可に加えて実行」を選ぶと、次を許可リストへ加えます(以後は承認なしで実行されます):")} <span className="font-mono">{approval.steps.join("、")}</span>
              </p>
            )}
          </>
        )}

        {approval.kind === "tool_use" && (
          <>
            {approval.reason && <p className="mt-2 text-muted-foreground">{approval.reason}</p>}
            <p className="mt-2 text-xs text-muted-foreground">
              {t("ツール:")} <span className="font-mono break-all">{approval.path}</span>
            </p>
            <pre className="mt-2 max-h-48 overflow-auto rounded-md border bg-background/60 p-2 font-mono text-xs whitespace-pre-wrap break-all">
              {approval.preview}
            </pre>
            <p className="mt-2 text-xs text-muted-foreground">{t("settings.json の permissions.ask に当たったため、確認しています。")}</p>
          </>
        )}

        {(approval.kind === "file" || approval.kind === "skill_update" || approval.kind === "skill_delete") && (
          <>
            <p className="mt-2 font-mono text-xs break-all">{approval.path}</p>
            <pre className="mt-2 max-h-48 overflow-auto rounded-md border bg-background/60 p-2 text-xs whitespace-pre-wrap">
              {approval.preview}
            </pre>
          </>
        )}

        <p className="mt-4 flex items-center gap-2 text-xs text-muted-foreground" aria-live="polite">
          {listening === "preparing" || listening === "transcribing" ? (
            <Loader2 className="size-3.5 animate-spin" />
          ) : (
            <Mic className={listening === "listening" ? "size-3.5 animate-pulse text-primary" : "size-3.5"} />
          )}
          {t((byVoice ? LISTENING_LABEL : BUTTON_ONLY_LABEL)[listening])}
        </p>
        <div className="mt-3 flex flex-wrap gap-2">
          {approval.kind === "allow_command" ? (
            <>
              <Button size="sm" onClick={() => onAnswer(true, true)}>
                <Check /> {t("今回だけ実行")}
              </Button>
              <Button size="sm" variant="outline" onClick={() => onAnswer(true)} title={t("以後、このコマンドは承認なしで実行されます")}>
                <ListPlus /> {t("許可に加えて実行")}
              </Button>
            </>
          ) : (
            <Button size="sm" onClick={() => onAnswer(true)}>
              <Check /> {t("実行する")}
            </Button>
          )}
          <Button size="sm" variant="outline" onClick={() => onAnswer(false)}>
            <X /> {t("中止")}
          </Button>
        </div>
      </div>
    </div>
  );
}
