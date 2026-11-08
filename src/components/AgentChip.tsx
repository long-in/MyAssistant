import { User } from "lucide-react";
import { cn } from "@/lib/utils";

/** エージェントの名称を、イメージカラーの丸と並べて示す。パーソナル(色なし)はアイコンを使う。 */
export default function AgentChip({ name, color, className }: { name: string; color: string | null; className?: string }) {
  return (
    <span className={cn("inline-flex min-w-0 items-center gap-2", className)}>
      {color ? (
        <span className="size-2.5 shrink-0 rounded-full" style={{ backgroundColor: color }} />
      ) : (
        <User className="size-3 shrink-0 text-muted-foreground" />
      )}
      <span className="truncate">{name}</span>
    </span>
  );
}
