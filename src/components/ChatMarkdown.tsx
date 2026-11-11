"use client";

import { Children, isValidElement, useEffect, useId, useState, type ReactNode } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Check, Copy } from "lucide-react";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/** クリップボードへ写す。許可されていない環境では、選択してコピーする昔ながらの方法で代える。 */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    const area = document.createElement("textarea");
    area.value = text;
    area.style.position = "fixed";
    area.style.opacity = "0";
    document.body.appendChild(area);
    area.select();
    const ok = document.execCommand("copy");
    area.remove();
    return ok;
  }
}

/** コピーボタン。押すと少しのあいだチェックに変わる。`label`を渡すと文字も添える。 */
export function CopyButton({ text, label, className }: { text: string; label?: string; className?: string }) {
  const t = useT();
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const id = setTimeout(() => setCopied(false), 1500);
    return () => clearTimeout(id);
  }, [copied]);
  return (
    <button
      type="button"
      aria-label={t("コピー")}
      title={copied ? t("コピーしました") : t("コピー")}
      onClick={() => void copyText(text).then(setCopied)}
      className={cn(
        "inline-flex items-center gap-1 rounded px-1.5 py-1 text-xs text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring focus-visible:outline-none",
        className,
      )}
    >
      {copied ? <Check className="size-3.5 text-primary" /> : <Copy className="size-3.5" />}
      {label && <span>{copied ? t("コピーしました") : label}</span>}
    </button>
  );
}

/** コードブロック。上の帯に言語名とコピーボタンを出す。 */
function CodeBlock({ lang, code, children }: { lang: string; code: string; children?: ReactNode }) {
  return (
    <div className="not-prose my-3 overflow-hidden rounded-md border bg-muted/40">
      <div className="flex items-center justify-between border-b bg-muted/60 pr-1 pl-3">
        <span className="font-mono text-[0.7rem] text-muted-foreground">{lang || "text"}</span>
        <div className="flex items-center">
          {children}
          <CopyButton text={code} />
        </div>
      </div>
      <pre className="overflow-x-auto p-3 font-mono text-xs leading-relaxed whitespace-pre">
        <code>{code}</code>
      </pre>
    </div>
  );
}

/** 図の描画を待つ時間(ms)。返答を少しずつ表示している間は、書きかけの図を描き直し続けない。 */
const MERMAID_DEBOUNCE_MS = 400;

/**
 * Mermaidの図。描けたら図を、描けない(書きかけ・文法の誤り)あいだは元のコードを見せる。
 * 図とコードは切り替えられ、どちらでも元のコードをコピーできる。ライブラリは最初の図で読み込む。
 */
function MermaidBlock({ code }: { code: string }) {
  const t = useT();
  const id = `mermaid-${useId().replace(/[^a-zA-Z0-9]/g, "")}`;
  const [svg, setSvg] = useState<{ code: string; svg: string } | null>(null);
  const [showCode, setShowCode] = useState(false);

  useEffect(() => {
    let alive = true;
    const timer = setTimeout(async () => {
      try {
        const mermaid = (await import("mermaid")).default;
        const dark = document.documentElement.classList.contains("dark");
        // `strict`は、図の中のHTMLとクリックの動作を無効にする(返答は信頼できない文章として扱う)
        mermaid.initialize({ startOnLoad: false, securityLevel: "strict", theme: dark ? "dark" : "default", fontFamily: "inherit" });
        if (!(await mermaid.parse(code, { suppressErrors: true }))) return;
        const out = await mermaid.render(id, code);
        if (alive) setSvg({ code, svg: out.svg });
      } catch {
        // 描けない図は、コードのまま見せる
      }
    }, MERMAID_DEBOUNCE_MS);
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [code, id]);

  const drawn = svg?.code === code ? svg.svg : null;
  const toggle = drawn && (
    <button
      type="button"
      onClick={() => setShowCode((v) => !v)}
      className="rounded px-1.5 py-1 text-xs text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
    >
      {showCode ? t("図を表示") : t("コードを表示")}
    </button>
  );
  if (!drawn || showCode) {
    return (
      <CodeBlock lang="mermaid" code={code}>
        {toggle}
      </CodeBlock>
    );
  }
  return (
    <div className="not-prose my-3 overflow-hidden rounded-md border bg-muted/40">
      <div className="flex items-center justify-between border-b bg-muted/60 pr-1 pl-3">
        <span className="font-mono text-[0.7rem] text-muted-foreground">mermaid</span>
        <div className="flex items-center">
          {toggle}
          <CopyButton text={code} />
        </div>
      </div>
      {/* Mermaidが`strict`で無害化したSVGだけを入れる */}
      <div className="flex justify-center overflow-x-auto p-3 [&_svg]:h-auto [&_svg]:max-w-full" dangerouslySetInnerHTML={{ __html: drawn }} />
    </div>
  );
}

/**
 * Markdownを描画する(表・リスト・コード・リンク・見出し・Mermaidの図)。チャットの返答と、ナレッジ・スキルの表示で使う。
 * - コードブロックと図には、コピーボタンを付ける。
 * - リンクは開かない(アプリ内の画面が遷移しないよう、文字だけを示す)。
 * - 画像は読み込まない。文章に混ざった指示で、URLに情報を載せて外へ送らせないため、代わりに説明だけを示す。
 */
export function ChatMarkdown({ text, className }: { text: string; className?: string }) {
  const t = useT();
  return (
    <div className={cn("prose prose-sm max-w-none break-words text-foreground dark:prose-invert [&_li]:whitespace-pre-line [&_p]:whitespace-pre-line", className)}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ children, href }) => (
            <a href={href} title={href} onClick={(e) => e.preventDefault()} className="cursor-default">
              {children}
            </a>
          ),
          img: ({ alt, src }) => (
            <span className="text-muted-foreground" title={typeof src === "string" ? src : undefined}>
              [{t("画像")}
              {alt ? `: ${alt}` : ""}]
            </span>
          ),
          table: ({ children }) => (
            <div className="overflow-x-auto">
              <table>{children}</table>
            </div>
          ),
          pre: ({ children }) => {
            const el = Children.toArray(children)[0];
            if (!isValidElement<{ className?: string; children?: ReactNode }>(el)) return <pre>{children}</pre>;
            const lang = /language-([\w+-]+)/.exec(el.props.className ?? "")?.[1] ?? "";
            const code = String(el.props.children ?? "").replace(/\n$/, "");
            return lang === "mermaid" ? <MermaidBlock code={code} /> : <CodeBlock lang={lang} code={code} />;
          },
        }}
      >
        {text}
      </ReactMarkdown>
    </div>
  );
}
