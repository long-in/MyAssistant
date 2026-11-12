/**
 * 画面では閲覧だけにする場所か(Rust の `knowledge::is_read_only` と同じ判定)。
 * ナレッジ直下の `data/`(担当システムのフォルダ)の配下と、`.` 始まりを含むパス(`.history/` など)。
 */
export const isReadOnlyPath = (path: string) => {
  const parts = path.replace(/^\.\//, "").split("/");
  return parts[0].toLowerCase() === "data" || parts.some((p) => p.startsWith("."));
};
