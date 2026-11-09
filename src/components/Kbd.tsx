/** ボタン内に添えるキーボードショートカットのヒント。 */
export function Kbd({ children }: { children: React.ReactNode }) {
  return <kbd className="ml-1 rounded border px-1 font-mono text-[10px] leading-4 opacity-60">{children}</kbd>;
}
