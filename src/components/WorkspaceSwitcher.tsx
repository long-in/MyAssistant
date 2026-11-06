"use client";

import { useRouter } from "next/navigation";
import { Briefcase, User } from "lucide-react";
import { toast } from "sonner";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { PERSONAL_ID, errorMessage } from "@/lib/api";
import { cn } from "@/lib/utils";
import { switchTo, useWorkspace } from "@/lib/workspace";

const LAST_SPACE_KEY = "lastSpace";

function lastSpace(): string | null {
  try {
    return localStorage.getItem(LAST_SPACE_KEY);
  } catch {
    return null;
  }
}

/** 左メニュー上部の、パーソナル / ワークの切り替えと、ワークのスペース選択。`collapsed`はアイコンだけの表示。 */
export default function WorkspaceSwitcher({ collapsed }: { collapsed: boolean }) {
  const router = useRouter();
  const { active, spaces } = useWorkspace();
  const personal = active.id === PERSONAL_ID;

  const go = (id: string) =>
    switchTo(id)
      .then(() => {
        if (id !== PERSONAL_ID) {
          try {
            localStorage.setItem(LAST_SPACE_KEY, id);
          } catch {}
        }
      })
      .catch((e) => toast.error(errorMessage(e)));

  const openWork = () => {
    if (!personal) return;
    const target = spaces.find((p) => p.id === lastSpace()) ?? spaces[0];
    if (target) void go(target.id);
    else router.push("/workspace"); // 先にスペースを作る
  };

  const items = spaces.map((p) => ({ value: p.id, label: p.name }));
  const modeButton = (on: boolean, onClick: () => void, label: string, icon: React.ReactNode) => (
    <button
      type="button"
      onClick={onClick}
      title={label}
      aria-label={label}
      aria-pressed={on}
      className={cn(
        "flex flex-1 items-center justify-center gap-1.5 rounded-sm px-2 py-1.5 text-xs font-medium transition-all",
        on ? "bg-primary/15 text-primary" : "text-muted-foreground hover:bg-primary/10 hover:text-foreground",
      )}
    >
      {icon}
      <span className={cn("hidden", !collapsed && "md:inline")}>{label}</span>
    </button>
  );

  return (
    <div className={cn("mb-4 flex w-full flex-col gap-2 px-1", collapsed ? "items-center" : "md:px-0")}>
      <div className={cn("flex w-full rounded-md border bg-background/50 p-0.5", collapsed ? "flex-col" : "flex-col md:flex-row")}>
        {modeButton(personal, () => void go(PERSONAL_ID), "Personal", <User className="size-3.5 shrink-0" />)}
        {modeButton(
          !personal,
          openWork,
          "Work",
          <Briefcase className="size-3.5 shrink-0" style={active.space ? { color: active.space.color } : undefined} />,
        )}
      </div>
      {!personal && spaces.length > 0 && (
        <Select items={items} value={active.id} onValueChange={(v) => v && void go(v as string)}>
          <SelectTrigger
            className={cn("hidden w-full", !collapsed && "md:flex")}
            aria-label="Spaces"
            style={{ borderColor: active.space?.color }}
          >
            <span className="size-2.5 shrink-0 rounded-full" style={{ backgroundColor: active.space?.color }} />
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {spaces.map((p) => (
              <SelectItem key={p.id} value={p.id}>
                <span className="size-2.5 shrink-0 rounded-full" style={{ backgroundColor: p.color }} />
                {p.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      )}
      {!personal && active.space && collapsed && spaces.length > 0 && (
        // 畳んだメニューでは、色の丸を押してスペースを選ぶ
        <Select items={items} value={active.id} onValueChange={(v) => v && void go(v as string)}>
          <SelectTrigger
            className="size-8 justify-center rounded-full p-0 [&>svg:last-child]:hidden"
            aria-label="Space"
            title={active.space.name}
            style={{ borderColor: active.space.color }}
          >
            <span className="size-3.5 rounded-full" style={{ backgroundColor: active.space.color }} />
          </SelectTrigger>
          <SelectContent>
            {spaces.map((p) => (
              <SelectItem key={p.id} value={p.id}>
                <span className="size-2.5 shrink-0 rounded-full" style={{ backgroundColor: p.color }} />
                {p.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      )}
    </div>
  );
}
