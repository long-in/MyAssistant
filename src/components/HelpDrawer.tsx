"use client";

import { useT } from "@/lib/i18n";
import { CircleHelp, X } from "lucide-react";
import { Dialog } from "@base-ui/react/dialog";
import { Button } from "@/components/ui/button";

/** 「?」ボタン。押すと画面右から詳しい説明のパネルが開く。本文は`children`。 */
export default function HelpDrawer({ title, children }: { title: string; children: React.ReactNode }) {
  const t = useT();
  return (
    <Dialog.Root>
      <Dialog.Trigger
        aria-label={t("{title}の説明", { title })}
        title={t("説明を見る")}
        className="inline-flex size-6 shrink-0 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-primary/10 hover:text-primary focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
      >
        <CircleHelp className="size-4" />
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Backdrop className="fixed inset-0 z-50 bg-black/40 duration-150 data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0" />
        <Dialog.Popup className="fixed inset-y-0 right-0 z-50 flex w-full max-w-md flex-col border-l bg-popover text-popover-foreground shadow-xl duration-200 data-open:animate-in data-open:slide-in-from-right data-closed:animate-out data-closed:slide-out-to-right">
          <div className="flex items-center justify-between gap-3 border-b px-5 py-4">
            <Dialog.Title className="text-base font-semibold">{title}</Dialog.Title>
            <Dialog.Close render={<Button type="button" variant="ghost" size="icon-sm" aria-label={t("閉じる")} />}>
              <X />
            </Dialog.Close>
          </div>
          <div className="flex-1 space-y-3 overflow-y-auto px-5 py-4 text-sm leading-relaxed text-muted-foreground [&_li]:ml-4 [&_li]:list-disc [&_strong]:text-foreground">
            {children}
          </div>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
