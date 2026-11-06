"use client";

import { useRouter } from "next/navigation";
import { Briefcase, User } from "lucide-react";
import { toast } from "sonner";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { PERSONAL_ID, errorMessage } from "@/lib/api";
import { cn } from "@/lib/utils";
import { switchTo, useWorkspace } from "@/lib/workspace";

const LAST_PROJECT_KEY = "lastProject";

function lastProject(): string | null {
  try {
    return localStorage.getItem(LAST_PROJECT_KEY);
  } catch {
    return null;
  }
}

/** 左メニュー上部の、パーソナル / ワークの切り替えと、ワークのプロジェクト選択。`collapsed`はアイコンだけの表示。 */
export default function WorkspaceSwitcher({ collapsed }: { collapsed: boolean }) {
  const router = useRouter();
  const { active, projects } = useWorkspace();
  const personal = active.id === PERSONAL_ID;

  const go = (id: string) =>
    switchTo(id)
      .then(() => {
        if (id !== PERSONAL_ID) {
          try {
            localStorage.setItem(LAST_PROJECT_KEY, id);
          } catch {}
        }
      })
      .catch((e) => toast.error(errorMessage(e)));

  const openWork = () => {
    if (!personal) return;
    const target = projects.find((p) => p.id === lastProject()) ?? projects[0];
    if (target) void go(target.id);
    else router.push("/workspace"); // 先にプロジェクトを作る
  };

  const items = projects.map((p) => ({ value: p.id, label: p.name }));
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
          <Briefcase className="size-3.5 shrink-0" style={active.project ? { color: active.project.color } : undefined} />,
        )}
      </div>
      {!personal && projects.length > 0 && (
        <Select items={items} value={active.id} onValueChange={(v) => v && void go(v as string)}>
          <SelectTrigger
            className={cn("hidden w-full", !collapsed && "md:flex")}
            aria-label="Projects"
            style={{ borderColor: active.project?.color }}
          >
            <span className="size-2.5 shrink-0 rounded-full" style={{ backgroundColor: active.project?.color }} />
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {projects.map((p) => (
              <SelectItem key={p.id} value={p.id}>
                <span className="size-2.5 shrink-0 rounded-full" style={{ backgroundColor: p.color }} />
                {p.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      )}
      {!personal && active.project && collapsed && projects.length > 0 && (
        // 畳んだメニューでは、色の丸を押してプロジェクトを選ぶ
        <Select items={items} value={active.id} onValueChange={(v) => v && void go(v as string)}>
          <SelectTrigger
            className="size-8 justify-center rounded-full p-0 [&>svg:last-child]:hidden"
            aria-label="Project"
            title={active.project.name}
            style={{ borderColor: active.project.color }}
          >
            <span className="size-3.5 rounded-full" style={{ backgroundColor: active.project.color }} />
          </SelectTrigger>
          <SelectContent>
            {projects.map((p) => (
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
