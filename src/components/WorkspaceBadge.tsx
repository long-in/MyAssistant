"use client";

import { Briefcase, ClipboardList, User } from "lucide-react";
import { LEADER_COLOR, LEADER_ID, LEADER_NAME } from "@/lib/api";
import { cn } from "@/lib/utils";
import { useWorkspace } from "@/lib/workspace";

/** いまどのエージェント(パーソナル / ワークのスペース)と話しているかを、名称とイメージカラーで示す。 */
export default function WorkspaceBadge({ className }: { className?: string }) {
  const { active } = useWorkspace();
  const leader = active.id === LEADER_ID;
  const space = leader ? { name: LEADER_NAME, color: LEADER_COLOR } : active.space;
  return (
    <div
      className={cn(
        "flex max-w-64 items-center gap-2 rounded-full border bg-background/80 px-3 py-1 text-xs font-medium backdrop-blur",
        className,
      )}
      style={space ? { borderColor: space.color } : undefined}
      title={leader ? LEADER_NAME : space ? `Work: ${space.name}` : "Personal"}
    >
      {leader ? (
        <ClipboardList className="size-3 shrink-0" style={{ color: LEADER_COLOR }} />
      ) : space ? (
        <span className="size-2.5 shrink-0 rounded-full" style={{ backgroundColor: space.color }} />
      ) : (
        <User className="size-3 shrink-0 text-muted-foreground" />
      )}
      <span className="truncate">{space ? space.name : "Personal"}</span>
      {space && !leader && <Briefcase className="size-3 shrink-0 text-muted-foreground" />}
    </div>
  );
}
