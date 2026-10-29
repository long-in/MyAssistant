"use client";

import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Bug, BookOpen, Keyboard, MessageSquare, Settings } from "lucide-react";
import { DEBUG_MODE_EVENT, getDebugMode, setTrayLanguage } from "@/lib/api";
import { useLang, useT } from "@/lib/i18n";
import { matchShortcut, type ShortcutId } from "@/lib/shortcuts";
import { cn } from "@/lib/utils";

const NAV = [
  { href: "/", label: "対話", icon: MessageSquare },
  { href: "/knowledge", label: "ナレッジ", icon: BookOpen },
  { href: "/setup", label: "設定", icon: Settings },
  { href: "/shortcuts", label: "ショートカット", icon: Keyboard },
] as const;

const DEBUG_NAV = { href: "/debug", label: "デバッグ", icon: Bug } as const;

const NAV_SHORTCUTS: Partial<Record<ShortcutId, string>> = {
  chat: "/",
  knowledge: "/knowledge",
  settings: "/setup",
  help: "/shortcuts",
};

export default function AppShell({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  const router = useRouter();
  const t = useT();
  const lang = useLang();
  const [debug, setDebug] = useState(false);

  useEffect(() => {
    document.documentElement.lang = lang;
    setTrayLanguage(lang).catch(() => {});
  }, [lang]);

  useEffect(() => {
    const load = () => getDebugMode().then(setDebug).catch(() => setDebug(false));
    load();
    window.addEventListener(DEBUG_MODE_EVENT, load);
    return () => window.removeEventListener(DEBUG_MODE_EVENT, load);
  }, []);

  useEffect(() => {
    const un = listen<string>("navigate", (e) => router.push(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, [router]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const id = matchShortcut(e);
      const dest = id && NAV_SHORTCUTS[id];
      if (dest) {
        e.preventDefault();
        router.push(dest);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [router]);

  return (
    <div className="flex h-full">
      <nav className="flex w-16 shrink-0 flex-col items-center gap-1 border-r bg-sidebar/80 py-4 backdrop-blur md:w-52 md:items-stretch md:px-3">
        <div className="mb-6 flex items-center justify-center gap-2 px-2 md:justify-start">
          <span className="size-2 shrink-0 rounded-full bg-primary shadow-[0_0_10px_var(--primary)]" />
          <span className="hud-label hidden text-[0.8rem] font-semibold text-foreground md:inline">MyAssistant</span>
        </div>
        {(debug ? [...NAV, DEBUG_NAV] : NAV).map(({ href, label, icon: Icon }) => {
          const active = href === "/" ? pathname === "/" : pathname.startsWith(href);
          return (
            <Link
              key={href}
              href={href}
              title={t(label)}
              className={cn(
                "relative flex items-center justify-center gap-2 rounded-sm px-3 py-2 text-sm font-medium text-muted-foreground transition-all hover:bg-primary/10 hover:text-foreground md:justify-start",
                active && "bg-primary/10 text-primary before:absolute before:inset-y-1 before:left-0 before:w-0.5 before:bg-primary before:shadow-[0_0_8px_var(--primary)]",
              )}
            >
              <Icon className="size-4 shrink-0" />
              <span className="hidden md:inline">{t(label)}</span>
            </Link>
          );
        })}
      </nav>
      <main className="min-w-0 flex-1 overflow-hidden">{children}</main>
    </div>
  );
}
