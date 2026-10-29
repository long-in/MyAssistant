"use client";

import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Bug, BookOpen, Keyboard, MessageSquare, PanelLeftClose, PanelLeftOpen, Settings } from "lucide-react";
import Chat from "@/components/Chat";
import { DEBUG_MODE_EVENT, getDebugMode, getKeyStatus, setTrayLanguage } from "@/lib/api";
import { useLang, useT } from "@/lib/i18n";
import { matchShortcut, type ShortcutId } from "@/lib/shortcuts";
import { cn } from "@/lib/utils";

const NAV = [
  { href: "/", label: "チャット", icon: MessageSquare },
  { href: "/knowledge", label: "ナレッジベース", icon: BookOpen },
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

const COLLAPSED_KEY = "sidebarCollapsed";

export default function AppShell({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  const router = useRouter();
  const t = useT();
  const lang = useLang();
  const [debug, setDebug] = useState(false);
  const [chatReady, setChatReady] = useState(false);
  const [modelSlot, setModelSlot] = useState<HTMLElement | null>(null);
  const [collapsed, setCollapsed] = useState(false);
  const onChat = pathname === "/";

  // 静的エクスポートのハイドレーション不一致を避けるため、保存値はマウント後に読む
  useEffect(() => {
    try {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setCollapsed(localStorage.getItem(COLLAPSED_KEY) === "1");
    } catch {}
  }, []);

  const toggleCollapsed = () => {
    setCollapsed((c) => {
      try {
        localStorage.setItem(COLLAPSED_KEY, c ? "0" : "1");
      } catch {}
      return !c;
    });
  };

  // 対話は画面移動で作り直さないよう、ここで常時マウントして非表示にするだけにする。APIキー登録後に初めて立ち上げる
  useEffect(() => {
    if (chatReady || !onChat) return;
    getKeyStatus()
      .then((status) => {
        if (Object.values(status).some(Boolean)) setChatReady(true);
        else router.replace("/setup");
      })
      .catch(() => router.replace("/setup"));
  }, [chatReady, onChat, router]);

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
      <nav
        className={cn(
          "flex w-16 shrink-0 flex-col items-center gap-1 border-r bg-sidebar/80 py-4 backdrop-blur",
          !collapsed && "md:w-52 md:items-stretch md:px-3",
        )}
      >
        <div className={cn("mb-6 flex items-center justify-center gap-2 px-2", !collapsed && "md:justify-start")}>
          <span className="size-2 shrink-0 rounded-full bg-primary shadow-[0_0_10px_var(--primary)]" />
          <span className={cn("hud-label hidden text-[0.8rem] font-semibold text-foreground", !collapsed && "md:inline")}>
            MyAssistant
          </span>
        </div>
        {(debug ? [...NAV, DEBUG_NAV] : NAV).map(({ href, label, icon: Icon }) => {
          const active = href === "/" ? pathname === "/" : pathname.startsWith(href);
          return (
            <Link
              key={href}
              href={href}
              title={t(label)}
              className={cn(
                "relative flex items-center justify-center gap-2 rounded-sm px-3 py-2 text-sm font-medium text-muted-foreground transition-all hover:bg-primary/10 hover:text-foreground",
                !collapsed && "md:justify-start",
                active && "bg-primary/10 text-primary before:absolute before:inset-y-1 before:left-0 before:w-0.5 before:bg-primary before:shadow-[0_0_8px_var(--primary)]",
              )}
            >
              <Icon className="size-4 shrink-0" />
              <span className={cn("hidden", !collapsed && "md:inline")}>{t(label)}</span>
            </Link>
          );
        })}
        <div ref={setModelSlot} className={cn("mt-auto hidden px-1 pt-4", !collapsed && "md:block")} />
        <button
          type="button"
          onClick={toggleCollapsed}
          title={t(collapsed ? "展開する" : "折りたたむ")}
          aria-label={t(collapsed ? "展開する" : "折りたたむ")}
          className={cn(
            "flex items-center justify-center gap-2 rounded-sm px-3 py-2 text-sm text-muted-foreground transition-all hover:bg-primary/10 hover:text-foreground",
            collapsed ? "mt-auto" : "mt-2 md:justify-start",
          )}
        >
          {collapsed ? <PanelLeftOpen className="size-4 shrink-0" /> : <PanelLeftClose className="size-4 shrink-0" />}
          <span className={cn("hidden", !collapsed && "md:inline")}>{t("折りたたむ")}</span>
        </button>
      </nav>
      <main className="min-w-0 flex-1 overflow-hidden">
        {chatReady && (
          <div className={cn("h-full", !onChat && "hidden")}>
            <Chat active={onChat} modelSlot={modelSlot} />
          </div>
        )}
        {!onChat && children}
      </main>
    </div>
  );
}
