<!-- BEGIN:nextjs-agent-rules -->

## This is NOT the Next.js you know

This version has breaking changes — APIs, conventions, and file structure may all differ from your training data. Read the relevant guide in `node_modules/next/dist/docs/` (resolved from this file's directory; in monorepos the `next` package may not be visible from the repo root) before writing any code. Heed deprecation notices.

This block is written and re-added by `next dev` — verify at `node_modules/next/dist/server/lib/generate-agent-files.js`. Removing it from a diff only re-creates the uncommitted change; committing it with your work keeps the tree clean.

<!-- END:nextjs-agent-rules -->

## Project: MyAssistant

Tauri 2 (Rust) + Next.js 静的エクスポート(`output: "export"`)のデスクトップアプリ。概要・構成は [README.md](README.md) を参照。

### 開発フロー(必須)

機能追加・修正(バグ修正を含む)では、次の順序を必ず守る。承諾なしにコードを変更しない。

1. **plan モードまたは仕様確認**: 実装前に plan モードで計画を提示するか、目的・挙動・影響範囲・未確定事項をユーザーへ確認する。
2. **ユーザーの承諾**: 計画または仕様に対して明示的な承諾を得る。承諾が無い、または内容が変わった場合は実装に移らず再確認する。
3. **実装**: 承諾された範囲内でのみ実装する。範囲外の変更が必要になった場合は、その時点で再度確認する。

- 軽微な誤字修正やドキュメントのみの変更など、挙動に影響しないものも、ユーザーが明示的に指示した内容に限って行う。

### ブランチ・コミット・PR

- ローカルでの並列開発を妨げないため、コード修正の依頼では作業ブランチを作成せず、基本的に `develop` ブランチ上で作業する。
- 専用の作業ブランチを作成するのは、ユーザーからコミットまたは PR 作成の指示があった場合に限る。
- 作業ブランチを作成するときは、先に `develop` を最新化(リモートから取得して取り込み)し、指示が無い限り最新化した `develop` から作業ブランチを作って `develop` へ PR を作成する。
- コミットを指示された場合は、機能単位でコミットを分ける。
- コミットメッセージは、修正内容を箇条書きで簡潔に記載する。
- PR の作成を指示された場合は、先に PR 本文をユーザーへ提示してレビューを受け、承諾を得てから実際に作成する。

### 構成上の制約

- フロントエンドは静的エクスポートのため、サーバー機能(API Routes、Server Actions、動的レンダリング等)は使えない。バックエンド処理は Rust 側の Tauri コマンドに実装し、`src/lib/api.ts` 経由で呼ぶ。
- Tauri コマンドを追加したら `src-tauri/src/lib.rs` の `generate_handler!` へ登録する。
- UI コンポーネントは shadcn/ui(`src/components/ui`、設定は `components.json`)。

### セキュリティ

- API キーの生値は `src-tauri/src/services/secrets.rs` の外へ出さない。取得関数(`get_key` / `get_tts_key`)をコマンドとして公開しない。フロントエンドには登録有無(bool)のみ返す。
- 例外: Bedrock の AWS 設定は、ユーザー本人が確認・編集するため `get_aws_files` コマンドで全文(credentials 含む)を返してよい。ログ・LLM・他のコマンドへは渡さない。画面では既定で伏せ字にする。
- ナレッジのパスはすべて `src-tauri/src/utils/path.rs` のサンドボックス(`resolve_existing` / `ensure_dir`)を通す。LLM 出力やフロントエンドからのパスを直接 `fs` に渡さない。
- `secrets.json`、`timeline.db`、`.env*` をコミットしない(実体はアプリデータディレクトリにある)。

### コマンド

```bash
npm run tauri dev      # 開発
npm run tauri build    # ビルド(macOS 最小バージョンは tauri.conf.json で 26.2)
npm run lint
cd src-tauri && cargo test
```

- `cargo` が PATH に無い場合は `~/.rustup/toolchains/stable-*/bin` を PATH に追加する(`~/.cargo/bin/rustup` が壊れている環境がある)。
- whisper.cpp のビルドには macOS 10.15 以上のデプロイターゲットが必要。`bundle.macOS.minimumSystemVersion` を下げない。
- 実 API を使うテストは `#[ignore]`。通常のテストでネットワークやキーを要求しない。

### 規約

- Rust のコメントは日本語の doc コメント(`//!` / `///`)で、既存コードの密度に合わせる。
