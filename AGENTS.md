<!-- BEGIN:nextjs-agent-rules -->

## This is NOT the Next.js you know

This version has breaking changes — APIs, conventions, and file structure may all differ from your training data. Read the relevant guide in `node_modules/next/dist/docs/` (resolved from this file's directory; in monorepos the `next` package may not be visible from the repo root) before writing any code. Heed deprecation notices.

This block is written and re-added by `next dev` — verify at `node_modules/next/dist/server/lib/generate-agent-files.js`. Removing it from a diff only re-creates the uncommitted change; committing it with your work keeps the tree clean.

<!-- END:nextjs-agent-rules -->

## Project: MyAssistant

Tauri 2 (Rust) + Next.js 静的エクスポート(`output: "export"`)のデスクトップアプリ。概要・構成は [README.md](README.md) を参照。

### 構成上の制約

- フロントエンドは静的エクスポートのため、サーバー機能(API Routes、Server Actions、動的レンダリング等)は使えない。バックエンド処理は Rust 側の Tauri コマンドに実装し、`src/lib/api.ts` 経由で呼ぶ。
- Tauri コマンドを追加したら `src-tauri/src/lib.rs` の `generate_handler!` へ登録する。
- UI コンポーネントは shadcn/ui(`src/components/ui`、設定は `components.json`)。

### セキュリティ

- API キーの生値は `src-tauri/src/services/secrets.rs` の外へ出さない。取得関数(`get_key` / `get_tts_key`)をコマンドとして公開しない。フロントエンドには登録有無(bool)のみ返す。
- ナレッジのパスはすべて `src-tauri/src/utils/path.rs` のサンドボックス(`resolve_existing` / `ensure_dir`)を通す。LLM 出力やフロントエンドからのパスを直接 `fs` に渡さない。
- `secrets.json`、`timeline.db`、`.env*` をコミットしない(実体はアプリデータディレクトリにある)。

### コマンド

```bash
npm run tauri dev      # 開発
npm run tauri build    # ビルド(macOS 最小バージョンは tauri.conf.json で 11.0)
npm run lint
cd src-tauri && cargo test
```

- `cargo` が PATH に無い場合は `~/.rustup/toolchains/stable-*/bin` を PATH に追加する(`~/.cargo/bin/rustup` が壊れている環境がある)。
- whisper.cpp のビルドには macOS 10.15 以上のデプロイターゲットが必要。`bundle.macOS.minimumSystemVersion` を下げない。
- 実 API を使うテストは `#[ignore]`。通常のテストでネットワークやキーを要求しない。

### 規約

- Rust のコメントは日本語の doc コメント(`//!` / `///`)で、既存コードの密度に合わせる。
