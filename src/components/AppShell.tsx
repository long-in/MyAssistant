"use client";

import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Bug, BookOpen, Library, ClipboardList, Globe, Keyboard, MessageSquare, PanelLeftClose, PanelLeftOpen, ScrollText, Settings } from "lucide-react";
import Chat from "@/components/Chat";
import AttentionToasts from "@/components/AttentionToasts";
import MfaModal from "@/components/MfaModal";
import SsoModal from "@/components/SsoModal";
import AgentPalette from "@/components/AgentPalette";
import AgentRail from "@/components/AgentRail";
import { DEBUG_MODE_EVENT, LEADER_COLOR, LEADER_ID, LEADER_NAME, PERSONAL_ID, getDebugMode, getKeyStatus } from "@/lib/api";
import { useLang } from "@/lib/i18n";
import { syncGlobalPrefs } from "@/lib/globalSync";
import { matchShortcut, type ShortcutId } from "@/lib/shortcuts";
import { cn } from "@/lib/utils";
import { startWorkspaceSync, useWorkspace } from "@/lib/workspace";

/** 主要な機能。 */
const MAIN_NAV = [
  { href: "/", label: "Chat", icon: MessageSquare },
  { href: "/knowledge", label: "Knowledge", icon: BookOpen },
  { href: "/skills", label: "Skills", icon: ScrollText },
  { href: "/setup", label: "Settings", icon: Settings },
] as const;

/** 補助的な項目(メニューの下側にまとめる)。 */
/** 全エージェントのナレッジ。エージェントに依存しないので補助の側に置く。 */
const ALL_KNOWLEDGE_NAV = { href: "/knowledge/all", label: "All Knowledge", icon: Library } as const;
const GLOBAL_NAV = { href: "/global", label: "Global Settings", icon: Globe } as const;
const HELP_NAV = { href: "/shortcuts", label: "Shortcuts", icon: Keyboard } as const;
/** デバッグ(デバッグモードがオンのときだけ)。ログはワークスペースごとなので、主要メニューに置く。 */
const DEBUG_NAV = { href: "/debug", label: "Debug", icon: Bug } as const;

const NAV_SHORTCUTS: Partial<Record<ShortcutId, string>> = {
  chat: "/",
  knowledge: "/knowledge",
  settings: "/setup",
  help: "/shortcuts",
};

/** アプリのバージョン(package.json からビルド時に埋め込まれる)。 */
const APP_VERSION = process.env.NEXT_PUBLIC_APP_VERSION ?? "";

const COLLAPSED_KEY = "sidebarCollapsed";

export default function AppShell({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  const router = useRouter();
  const lang = useLang();
  const [debug, setDebug] = useState(false);
  const [chatReady, setChatReady] = useState(false);
  const [collapsed, setCollapsed] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const onChat = pathname === "/";
  const { active, settingsVersion } = useWorkspace();
  const workspaceId = active.id;
  const personal = workspaceId === PERSONAL_ID;
  const leader = workspaceId === LEADER_ID;
  const accent = leader ? LEADER_COLOR : active.agent?.color;
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

  // エージェントのイメージカラーを、画面全体で参照できるようにする
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
    const here = href === "/" ? pathname === "/" : pathname.replace(/\/$/, "") === href;
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
      <AgentRail />
      <nav
        className={cn(
          "flex w-14 shrink-0 flex-col items-center gap-1 border-r bg-sidebar/70 py-3 backdrop-blur",
          !collapsed && "md:w-48 md:items-stretch md:px-3",
        )}
      >
        {/* アプリ名とバージョン。稼働ランプは点滅させる */}
        <div className={cn("mb-1 flex w-full items-center justify-center gap-2 py-1", !collapsed && "md:justify-start md:px-1")} title={`MyAssistant v${APP_VERSION}`}>
          <span className="animate-hud-blink-slow size-2 shrink-0 bg-primary neon-glow" aria-hidden />
          <span className={cn("hidden min-w-0 flex-col leading-tight", !collapsed && "md:flex")}>
            <span className="hud-label truncate text-[0.7rem] font-semibold">MyAssistant</span>
            <span className="font-mono text-[0.6rem] tracking-widest text-muted-foreground">{`v${APP_VERSION} // ONLINE`}</span>
          </span>
        </div>
        {/* ワークのときだけ、どのエージェントかを色付きで示す(パーソナルは表示しない) */}
        {!personal && (
          <div
            className={cn("mb-2 flex w-full items-center justify-center rounded-md border px-2 py-2", !collapsed && "md:justify-start md:gap-2")}
            style={accent ? { borderColor: accent, backgroundColor: `${accent}14` } : undefined}
            title={`Work / ${leader ? LEADER_NAME : (active.agent?.name ?? "")}`}
          >
            {leader ? <ClipboardList className="size-4 shrink-0" style={{ color: accent }} /> : <span className="size-3 shrink-0 rounded-full" style={{ backgroundColor: accent }} />}
            <span className={cn("hidden min-w-0 flex-col leading-tight", !collapsed && "md:flex")}>
              <span className="hud-label text-[0.65rem] text-muted-foreground">WORK</span>
              <span className="truncate text-sm font-semibold">{leader ? LEADER_NAME : (active.agent?.name ?? "")}</span>
            </span>
          </div>
        )}
        <div className="mb-1 h-px w-full bg-border" />
        {[...MAIN_NAV, ...(debug ? [DEBUG_NAV] : [])].map((n) => navLink(n))}
        <div className="my-2 h-px w-full bg-border" />
        {[ALL_KNOWLEDGE_NAV, GLOBAL_NAV, HELP_NAV].map((n) => navLink(n, true))}
        <button
          type="button"
          onClick={toggleCollapsed}
          title={collapsed ? "Expand" : "Collapse"}
          aria-label={collapsed ? "Expand" : "Collapse"}
          className={cn(
            "flex items-center justify-center gap-2 rounded-sm px-3 py-2 text-sm text-muted-foreground transition-all hover:bg-primary/10 hover:text-foreground",
            "mt-auto", !collapsed && "md:justify-start",
          )}
        >
          {collapsed ? <PanelLeftOpen className="size-4 shrink-0" /> : <PanelLeftClose className="size-4 shrink-0" />}
          <span className={cn("hidden", !collapsed && "md:inline")}>Collapse</span>
        </button>
      </nav>
      <main className="min-w-0 flex-1 overflow-hidden">
        {chatReady && (
          <div className={cn("h-full", !onChat && "hidden")}>
            <Chat key={workspaceId} workspaceId={workspaceId} active={onChat} />
          </div>
        )}
        {!onChat && <div key={`${workspaceId}:${settingsVersion}`} className="h-full">{children}</div>}
      </main>
      {paletteOpen && <AgentPalette onClose={() => setPaletteOpen(false)} />}
      <MfaModal />
      <SsoModal />
      <AttentionToasts />
    </div>
  );
}
