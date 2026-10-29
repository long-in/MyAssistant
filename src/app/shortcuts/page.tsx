"use client";

import { useT } from "@/lib/i18n";
import { useSyncExternalStore } from "react";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { SHORTCUTS, isMac, keyLabel } from "@/lib/shortcuts";

function Kbd({ children }: { children: React.ReactNode }) {
  return (
    <kbd className="inline-flex min-w-7 items-center justify-center rounded-md border bg-muted px-2 py-1 font-mono text-xs font-medium shadow-[0_1px_0_var(--border)]">
      {children}
    </kbd>
  );
}

const subscribe = () => () => {};

export default function Shortcuts() {
  const t = useT();
  const mac = useSyncExternalStore(subscribe, isMac, () => false);
  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto flex max-w-2xl flex-col gap-5 px-6 py-10">
        <div>
          <p className="hud-label mb-1">{"// Shortcuts"}</p>
          <h1 className="text-2xl font-semibold tracking-tight">{t("ショートカット")}</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            {t("キーボードだけでマイク入力や画面移動ができます。アプリのウィンドウがアクティブなときに使えます。")}
          </p>
        </div>

        <Card>
          <CardHeader>
            <CardTitle>{t("キーボードショートカット")}</CardTitle>
            <CardDescription>
              {mac ? t("⌘ は Command キーです。") : t("Ctrl キーを使います。macOS では ⌘ (Command) になります。")}
            </CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col divide-y">
            {SHORTCUTS.map((s) => (
              <div key={s.id} className="flex items-center justify-between gap-4 py-3">
                <div className="min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-medium">{t(s.label)}</span>
                    <Badge variant="secondary">{t(s.scope)}</Badge>
                  </div>
                  <p className="mt-0.5 text-sm text-muted-foreground">{t(s.description)}</p>
                </div>
                <div className="flex shrink-0 items-center gap-1">
                  {s.keys.map((k, i) => (
                    <Kbd key={i}>{keyLabel(k, mac)}</Kbd>
                  ))}
                </div>
              </div>
            ))}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>{t("音声入力の使い方")}</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-2 text-sm text-muted-foreground">
            <p>{t("1. マイクのショートカット(またはマイクボタン)で録音を開始します。録音中は下部の波形が動き、ボタンが赤く点滅します。")}</p>
            <p>{t("2. もう一度押すと録音を止め、ローカルで文字起こしして、そのまま送信されます。キーボードで文字を入力する場合は、下部の入力欄に書いてEnterで送信します。")}</p>
            <p>{t("3. 応答は「音声応答」がオンなら自動で読み上げられます。")}</p>
            <p>{t("初回のみ、音声認識モデルのダウンロードとマイクの使用許可が必要です。")}</p>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
