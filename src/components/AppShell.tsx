"use client";

import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Bug, BookOpen, FolderKanban, Globe, Keyboard, MessageSquare, PanelLeftClose, PanelLeftOpen, Settings, User } from "lucide-react";
import Chat from "@/components/Chat";
import AttentionToasts from "@/components/AttentionToasts";
import MfaModal from "@/components/MfaModal";
import SpacePalette from "@/components/SpacePalette";
import SpaceRail from "@/components/SpaceRail";
import { DEBUG_MODE_EVENT, PERSONAL_ID, getDebugMode, getKeyStatus } from "@/lib/api";
import { useLang } from "@/lib/i18n";
import { syncGlobalPrefs } from "@/lib/globalSync";
import { matchShortcut, type ShortcutId } from "@/lib/shortcuts";
import { cn } from "@/lib/utils";
import { startWorkspaceSync, useWorkspace } from "@/lib/workspace";

/** 主要な機能。 */
const MAIN_NAV = [
  { href: "/", label: "Chat", icon: MessageSquare },
  { href: "/knowledge", label: "Knowledge base", icon: BookOpen },
  { href: "/setup", label: "Settings", icon: Settings },
] as const;

/** 補助的な項目(メニューの下側にまとめる)。 */
const SPACES_NAV = { href: "/workspace", label: "Manage spaces", icon: FolderKanban } as const;
const GLOBAL_NAV = { href: "/global", label: "Global Settings", icon: Globe } as const;
const HELP_NAV = { href: "/shortcuts", label: "Shortcuts", icon: Keyboard } as const;
const DEBUG_NAV = { href: "/debug", label: "Debug", icon: Bug } as const;

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
  const lang = useLang();
  const [debug, setDebug] = useState(false);
  const [chatReady, setChatReady] = useState(false);
  const [modelSlot, setModelSlot] = useState<HTMLElement | null>(null);
  const [collapsed, setCollapsed] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const onChat = pathname === "/";
  const { active, settingsVersion } = useWorkspace();
  const accent = active.space?.color;
  const workspaceId = active.id;
  const personal = workspaceId === PERSONAL_ID;
  const prevWorkspace = useRef(workspaceId);

  useEffect(() => {
    startWorkspaceSync();
    // 表示言語とOS通知は全体設定。起動時に取り込む
    syncGlobalPrefs().catch(() => {});
  }, []);

  // ワークスペースを切り替えたら、対話はキーで作り直し(会話・音声入力・待機を止める)、APIキーの有無を切替先で確かめ直す
  useEffect(() => {
    if (prevWorkspace.current === workspaceId) return;
    prevWorkspace.current = workspaceId;
    setChatReady(false);
  }, [workspaceId]);

  // スペースのイメージカラーを、画面全体で参照できるようにする
  useEffect(() => {
    const root = document.documentElement;
    if (accent) root.style.setProperty("--workspace-color", accent);
    else root.style.removeProperty("--workspace-color");
  }, [accent]);

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
      if (id === "palette") {
        e.preventDefault();
        if (!e.repeat) setPaletteOpen((o) => !o);
        return;
      }
      const dest = id && NAV_SHORTCUTS[id];
      if (dest) {
        e.preventDefault();
        router.push(dest);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [router]);

  const navLink = ({ href, label, icon: Icon }: { href: string; label: string; icon: typeof Bug }, secondary = false) => {
    const here = href === "/" ? pathname === "/" : pathname.startsWith(href);
    return (
      <Link
        key={href}
        href={href}
        title={label}
        aria-current={here ? "page" : undefined}
        className={cn(
          "relative flex w-full items-center justify-center gap-2 rounded-sm px-3 text-sm font-medium text-muted-foreground transition-all hover:bg-primary/10 hover:text-foreground",
          secondary ? "py-1.5 text-xs" : "py-2",
          !collapsed && "md:justify-start",
          here && "bg-primary/10 text-primary before:absolute before:inset-y-1 before:left-0 before:w-0.5 before:bg-current",
        )}
        style={here && accent ? { color: accent, backgroundColor: `${accent}1f` } : undefined}
      >
        <Icon className="size-4 shrink-0" />
        <span className={cn("hidden", !collapsed && "md:inline")}>{label}</span>
      </Link>
    );
  };

  return (
    <div className="relative flex h-full">
      {accent && <div className="pointer-events-none absolute inset-x-0 top-0 z-50 h-0.5" style={{ backgroundColor: accent }} />}
      <SpaceRail />
      <nav
        className={cn(
          "flex w-14 shrink-0 flex-col items-center gap-1 border-r bg-sidebar/70 py-3 backdrop-blur",
          !collapsed && "md:w-48 md:items-stretch md:px-3",
        )}
      >
        {/* いまどこにいるか(パーソナルか、どのスペースか) */}
        <div
          className={cn("mb-3 flex w-full items-center justify-center rounded-md border px-2 py-2", !collapsed && "md:justify-start md:gap-2")}
          style={accent ? { borderColor: accent, backgroundColor: `${accent}14` } : undefined}
          title={personal ? "Personal" : `Work / ${active.space?.name ?? ""}`}
        >
          {personal ? (
            <User className="size-4 shrink-0 text-muted-foreground" />
          ) : (
            <span className="size-3 shrink-0 rounded-full" style={{ backgroundColor: accent }} />
          )}
          <span className={cn("hidden min-w-0 flex-col leading-tight", !collapsed && "md:flex")}>
            <span className="hud-label text-[0.65rem] text-muted-foreground">{personal ? "PERSONAL" : "WORK"}</span>
            <span className="truncate text-sm font-semibold">{personal ? "Personal" : (active.space?.name ?? "")}</span>
          </span>
        </div>
        {MAIN_NAV.map((n) => navLink(n))}
        <div className="my-2 h-px w-full bg-border" />
        {[...(!personal ? [SPACES_NAV] : []), GLOBAL_NAV, HELP_NAV, ...(debug ? [DEBUG_NAV] : [])].map((n) => navLink(n, true))}
        <div ref={setModelSlot} className={cn("mt-auto hidden px-1 pt-4", !collapsed && "md:block")} />
        <button
          type="button"
          onClick={toggleCollapsed}
          title={collapsed ? "Expand" : "Collapse"}
          aria-label={collapsed ? "Expand" : "Collapse"}
          className={cn(
            "flex items-center justify-center gap-2 rounded-sm px-3 py-2 text-sm text-muted-foreground transition-all hover:bg-primary/10 hover:text-foreground",
            collapsed ? "mt-auto" : "mt-2 md:justify-start",
          )}
        >
          {collapsed ? <PanelLeftOpen className="size-4 shrink-0" /> : <PanelLeftClose className="size-4 shrink-0" />}
          <span className={cn("hidden", !collapsed && "md:inline")}>Collapse</span>
        </button>
      </nav>
      <main className="min-w-0 flex-1 overflow-hidden">
        {chatReady && (
          <div className={cn("h-full", !onChat && "hidden")}>
            <Chat key={workspaceId} workspaceId={workspaceId} active={onChat} modelSlot={modelSlot} />
          </div>
        )}
        {!onChat && <div key={`${workspaceId}:${settingsVersion}`} className="h-full">{children}</div>}
      </main>
      {paletteOpen && <SpacePalette onClose={() => setPaletteOpen(false)} />}
      <MfaModal />
      <AttentionToasts />
    </div>
  );
}
