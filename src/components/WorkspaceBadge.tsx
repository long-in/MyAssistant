"use client";

import { Briefcase, User } from "lucide-react";
import { cn } from "@/lib/utils";
import { useWorkspace } from "@/lib/workspace";

/** いまどのエージェント(パーソナル / ワークのプロジェクト)と話しているかを、名称とイメージカラーで示す。 */
export default function WorkspaceBadge({ className }: { className?: string }) {
  const { active } = useWorkspace();
  const project = active.project;
  return (
    <div
      className={cn(
        "flex max-w-64 items-center gap-2 rounded-full border bg-background/80 px-3 py-1 text-xs font-medium backdrop-blur",
        className,
      )}
      style={project ? { borderColor: project.color } : undefined}
      title={project ? `Work: ${project.name}` : "Personal"}
    >
      {project ? (
        <span className="size-2.5 shrink-0 rounded-full" style={{ backgroundColor: project.color }} />
      ) : (
        <User className="size-3 shrink-0 text-muted-foreground" />
      )}
      <span className="truncate">{project ? project.name : "Personal"}</span>
      {project && <Briefcase className="size-3 shrink-0 text-muted-foreground" />}
    </div>
  );
}
