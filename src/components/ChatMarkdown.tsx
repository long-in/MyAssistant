"use client";

import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

/**
 * チャットの返答をMarkdownで描画する(表・リスト・コード・リンク・見出し)。
 * 改行はそのまま折り返す。リンクは開かない(アプリ内の画面が遷移しないよう、文字だけを示す)。
 */
export function ChatMarkdown({ text }: { text: string }) {
  return (
    <div className="prose prose-sm max-w-none break-words text-foreground dark:prose-invert [&_li]:whitespace-pre-line [&_p]:whitespace-pre-line">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ children, href }) => (
            <a href={href} title={href} onClick={(e) => e.preventDefault()} className="cursor-default">
              {children}
            </a>
          ),
          table: ({ children }) => (
            <div className="overflow-x-auto">
              <table>{children}</table>
            </div>
          ),
        }}
      >
        {text}
      </ReactMarkdown>
    </div>
  );
}
