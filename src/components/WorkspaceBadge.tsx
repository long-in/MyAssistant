"use client";

import { Briefcase, User } from "lucide-react";
import { cn } from "@/lib/utils";
import { useWorkspace } from "@/lib/workspace";

/** いまどのエージェント(パーソナル / ワークのスペース)と話しているかを、名称とイメージカラーで示す。 */
export default function WorkspaceBadge({ className }: { className?: string }) {
  const { active } = useWorkspace();
  const space = active.space;
  return (
    <div
      className={cn(
        "flex max-w-64 items-center gap-2 rounded-full border bg-background/80 px-3 py-1 text-xs font-medium backdrop-blur",
        className,
      )}
      style={space ? { borderColor: space.color } : undefined}
      title={space ? `Work: ${space.name}` : "Personal"}
    >
      {space ? (
        <span className="size-2.5 shrink-0 rounded-full" style={{ backgroundColor: space.color }} />
      ) : (
        <User className="size-3 shrink-0 text-muted-foreground" />
      )}
      <span className="truncate">{space ? space.name : "Personal"}</span>
      {space && <Briefcase className="size-3 shrink-0 text-muted-foreground" />}
    </div>
  );
}
