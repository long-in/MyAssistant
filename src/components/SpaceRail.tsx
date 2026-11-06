"use client";

import { useRouter } from "next/navigation";
import { Plus, User } from "lucide-react";
import { toast } from "sonner";
import { PERSONAL_ID, errorMessage } from "@/lib/api";
import { cn } from "@/lib/utils";
import { switchTo, useWorkspace } from "@/lib/workspace";

/** スペース名の頭文字(絵文字などのサロゲートペアも1文字として扱う)。 */
const initial = (name: string) => Array.from(name.trim())[0]?.toUpperCase() ?? "?";

/**
 * 左端のスペースレール。パーソナルと各スペースを、色付きのアイコンで常に並べて見せ、1クリックで切り替える。
 * メニューを畳んでも幅は変わらず、いま開いているスペースと、ほかのスペースへの切り替えが常に見える。
 */
export default function SpaceRail() {
  const router = useRouter();
  const { active, spaces } = useWorkspace();

  const go = (id: string) => {
    if (id === active.id) return;
    switchTo(id).catch((e) => toast.error(errorMessage(e)));
  };

  const item = (id: string, label: string, color: string | null, child: React.ReactNode) => {
    const on = active.id === id;
    return (
      <button
        key={id}
        type="button"
        onClick={() => go(id)}
        title={label}
        aria-label={label}
        aria-current={on ? "true" : undefined}
        className="group relative flex w-full items-center justify-center py-0.5"
      >
        {/* 選択中の印(左のバー) */}
        <span
          className={cn("absolute left-0 w-1 rounded-r-full transition-all", on ? "h-8 opacity-100" : "h-2 opacity-0 group-hover:opacity-60")}
          style={{ backgroundColor: color ?? "var(--primary)" }}
        />
        <span
          className={cn(
            "flex size-9 items-center justify-center text-sm font-semibold text-white transition-all",
            color ? "rounded-full" : "rounded-lg bg-muted text-foreground",
            on ? "ring-2 ring-offset-2 ring-offset-sidebar" : "opacity-80 group-hover:opacity-100",
          )}
          style={color ? { backgroundColor: color, ["--tw-ring-color" as string]: color } : on ? { ["--tw-ring-color" as string]: "var(--primary)" } : undefined}
        >
          {child}
        </span>
      </button>
    );
  };

  return (
    <div className="flex w-14 shrink-0 flex-col items-center gap-1 border-r bg-sidebar/90 py-3 backdrop-blur" aria-label="Spaces">
      {item(PERSONAL_ID, "Personal", null, <User className="size-4" />)}
      {spaces.length > 0 && <div className="my-1 h-px w-6 bg-border" />}
      <div className="flex min-h-0 w-full flex-1 flex-col items-center gap-1 overflow-y-auto overflow-x-hidden">
        {spaces.map((p) => item(p.id, p.name, p.color, initial(p.name)))}
      </div>
      <button
        type="button"
        onClick={() => router.push("/workspace")}
        title="Create or manage spaces"
        aria-label="Create or manage spaces"
        className="mt-1 flex size-9 shrink-0 items-center justify-center rounded-full border border-dashed text-muted-foreground transition-colors hover:border-primary hover:text-primary"
      >
        <Plus className="size-4" />
      </button>
    </div>
  );
}
