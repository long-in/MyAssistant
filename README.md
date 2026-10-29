# MyAssistant

音声で対話できるデスクトップ AI エージェント。会話はナレッジ(Markdown)として蓄積され、次の会話で関連する内容が自動で想起されます。ナレッジの閲覧や作成、インターネット検索、コマンドによる作業も、声で依頼して進められます。

- **フロントエンド:** Next.js 16 (静的エクスポート) / React 19 / Tailwind CSS 4 / shadcn/ui
- **バックエンド:** Tauri 2 (Rust)
- **対応 OS:** macOS 専用(11.0 以上)。Windows / Linux は未対応です(コマンド実行が `/bin/sh` 前提、音声認識が Metal 前提のため)。

機能ごとの詳細設計は [docs/](docs/README.md) を参照してください。

## 機能

### 対話

- **LLM 対話:** Anthropic / OpenAI に対応。使うモデルは画面で選びます。応答は読み上げ向けの短い話し言葉で返り、読み上げ後は自動で音声入力が再開して会話を続けられます。詳細は [chat-flow.md](docs/chat-flow.md)。
- **会話の継続と切り替え:** 会話は、切り替えを明示しない限り継続します(放置しても切り替わりません)。「新しい会話」ボタン、または「話題を変えて」などの発話に「はい」と答えたときだけ切り替わります。長くなった会話は自動で要約し、毎回送るトークンを抑えます。詳細は [conversation-session.md](docs/conversation-session.md)。
- **パーソナルデータ(オプション):** 名前・呼ばれ方・年齢・居住地などを手動で登録すると、呼びかけや回答の調整に使います(会話からの自動抽出はしません)。詳細は [user-profile.md](docs/user-profile.md)。

### 音声

- **音声入力 (STT):** ローカルの Whisper(既定。`ggml-large-v3-turbo-q5_0.bin` 約574MB、または `ggml-small.bin` 約466MB。初回にダウンロード)、Google Cloud Speech-to-Text、Aqua Voice から選べます。Silero VAD で声のない入力を除き、無音への幻聴を防ぎます。詳細は [voice-input.md](docs/voice-input.md)。
- **文字起こしの校正(オプション):** 設定でモデルを選ぶと、会話の文脈と訂正履歴から聞き間違いを直してから送信します。字幕から聞き取りを手で訂正すると、ナレッジに記録されて次の発話から反映されます。
- **音声出力 (TTS):** Google Cloud Text-to-Speech(日本語 / 英語を自動判別)、または端末内で合成する VOICEVOX CORE(キー不要・無料。設定画面から約90MBを取得)から選べます。字幕は読み上げの再生位置に合わせて表示されます。キーが無ければ字幕だけで応答します。詳細は [voice-output.md](docs/voice-output.md)。
- **常時音声待機(オプション):** 起動ワードを登録すると、対話画面を開いている間マイクを開いたままにし、起動ワードを含む発話があったときだけ依頼を受け付けます。含まない発話は LLM へ送られません。詳細は [wake-standby.md](docs/wake-standby.md)。

### ナレッジ

- **蓄積と整理:** 会話は即座に「キャプチャ」として Markdown に残り、未処理のログは設定した間隔(既定30分。アプリ終了時と「今すぐ整理」でも実行)で LLM がトピック別のノートへ整理します。保存先は設定で変更でき、Obsidian の vault としても開けます。詳細は [knowledge.md](docs/knowledge.md)。
- **関連ノートの想起:** ノートをタグ・リンク・同じ会話由来のつながりでグラフにし、発話に関連するノートを最大2ホップ辿って要約をプロンプトへ渡します(LLM・ネットワーク不使用)。詳細は [knowledge-graph.md](docs/knowledge-graph.md)。

### 作業の依頼(Claude 選択時)

- **ファイル操作:** ナレッジの一覧・読み取り・検索・新規作成(作成は承認制、上書き不可)。詳細は [agent-file-tools.md](docs/agent-file-tools.md)。
- **インターネット検索:** Anthropic のサーバーツールによる検索・ページ取得。検索ごとに追加料金がかかるため、設定でオフにでき、回数の上限も設定できます。
- **コマンド実行:** 「〜してください」という依頼に手順を提示し、音声で「はい」と承認すると実行して進めます。`rm`・`sudo`・OS に影響するコマンドは常に拒否し、読み取り系以外のコマンドはナレッジのディレクトリの中でしか実行できません。許可・拒否・都度承認のコマンドは設定画面の Command Policy で編集できます(実体はナレッジ内の `.tachicoma/command-policy.json`)。詳細は [agent-command-tasks.md](docs/agent-command-tasks.md) と [command-policy.md](docs/command-policy.md)。

### その他

- **使用量表示:** プロバイダー・モデル別の当月のトークン使用量と概算金額を表示します。
- **メニューバー常駐:** ウィンドウを閉じてもメニューバーに残り、アイコンのクリックで再表示します。終了はメニューから。詳細は [frontend.md](docs/frontend.md)。
- **バックアップ:** 設定(API キーを除く)を JSON でエクスポート・インポートできます。詳細は [data-and-settings.md](docs/data-and-settings.md)。
- **デバッグモード(オプション):** API の通信と文字起こしをナレッジ配下に記録し、画面で確認できます。詳細は [debug-log.md](docs/debug-log.md)。

### 対応モデル

| プロバイダー | 軽量 | 高性能 |
| --- | --- | --- |
| Anthropic | Claude Haiku 5.5 | Claude Sonnet 5.5 / Claude Opus 5.5 |
| OpenAI | GPT-4o mini | GPT-4o |

ファイル操作・コマンド実行・インターネット検索は Claude 選択時のみ使えます。

### キーボードショートカット

`Mod` は macOS では ⌘、それ以外では Ctrl です。

| キー | 動作 |
| --- | --- |
| `Space`(押している間) | 録音して、離すと文字起こし・送信(対話画面。入力欄フォーカス時は通常のスペース) |
| `Mod`+`Shift`+`M` | 録音の開始 / 停止 |
| `Mod`+`1` / `2` / `3` | 対話 / ナレッジ / 設定を開く |
| `Mod`+`/` | ショートカット一覧を開く |

## 必要環境

- Node.js と npm
- Rust (stable、1.90 以上)
- macOS(必須。Windows / Linux では動作しません)
- Xcode Command Line Tools(whisper.cpp のビルドに C++ コンパイラと CMake が必要)

## 開発

```bash
npm install
npm run tauri dev
```

Next.js のみを起動する場合は `npm run dev` です(Tauri のコマンドは使えません)。

```bash
npm run lint
cd src-tauri && cargo test
```

`cargo` が PATH に無い場合は `~/.rustup/toolchains/stable-*/bin` を PATH に追加してください。

## ビルド

```bash
npm run tauri build
```

成果物は `src-tauri/target/release/bundle/` 配下に出力されます。macOS の最小対応バージョンは 11.0 です([src-tauri/tauri.conf.json](src-tauri/tauri.conf.json))。whisper.cpp のビルドには macOS 10.15 以上のデプロイターゲットが必要なため、これを下げないでください。

## 初期設定

アプリの設定画面で API キーを登録します。

| 用途 | キー |
| --- | --- |
| Claude | Anthropic API キー |
| GPT | OpenAI API キー |
| 音声出力 / Google STT | Google Cloud API キー |
| Aqua Voice(STT) | Aqua Voice API キー |

登録時に疎通確認を行い、成功したときだけ保存します。ローカル Whisper を使う場合は、設定画面からモデルをダウンロードしてください。初回の録音時にマイクの使用許可を求められます。

常時音声待機や会話の切り替えの確認で音声を使うには、設定画面の「定型音声を作成」(選んだ読み上げエンジンで合成)を先に実行してください。

## データの保存場所

すべてアプリデータディレクトリ(識別子 `dev.longin.tachicoma-agent`)に保存され、リポジトリには含まれません。macOS では `~/Library/Application Support/dev.longin.tachicoma-agent/` です。識別子は保存先を変えないため、アプリ名の変更後も旧名のままにしています。

| パス | 内容 |
| --- | --- |
| `timeline.db` | SQLite。会話ログ・使用量・アプリ設定 |
| `secrets.json` | API キー。**平文 JSON**(権限 0600)。OS のキーチェーンは使っていません |
| `models/` | Whisper モデル |
| `voicevox/` | VOICEVOX CORE・辞書・音声モデル(任意) |
| `voice/` | 定型音声 |

ナレッジの Markdown は、既定ではアプリデータ配下に保存されます。設定画面またはナレッジ画面で任意のディレクトリに変更できます(既存のノートは移動しません)。

## ディレクトリ構成

```text
src/                    Next.js フロントエンド
  app/                  画面 (対話 / knowledge / setup / shortcuts / debug)
  components/           UI コンポーネント (shadcn/ui は components/ui)
  lib/                  Tauri 呼び出し、音声での承認・切り替えの判定、字幕の同期、ショートカット、料金計算
src-tauri/              Tauri バックエンド (Rust)
  src/commands/         フロントエンドから呼ぶコマンド
  src/services/         LLM クライアント、STT/TTS、整理バッチ、ナレッジ、エージェント、コマンド判定、キー保存
  src/db/               SQLite
  src/utils/            エラー型、パスのサンドボックス
  src/tray.rs           メニューバー常駐
  resources/            同梱モデル (Silero VAD)
public/sounds/          効果音
docs/                   機能ごとの詳細設計
```

## セキュリティ

- API キーの生値は `src-tauri/src/services/secrets.rs` の外へ出しません。フロントエンドには登録有無だけ返します。
- ナレッジのパスはすべて `src-tauri/src/utils/path.rs` のサンドボックスを通します。
- `secrets.json`、`timeline.db`、`.env*` はコミットしないでください。

詳細は [docs/security.md](docs/security.md) を参照してください。

## テスト

```bash
cd src-tauri
cargo test
# 実 API を使うテストは #[ignore] 済み。キーを用意して明示的に実行する
cargo test -- --ignored
```

## ライセンス

[MIT](LICENSE)。同梱モデルの表記は [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)、貢献の方法は [CONTRIBUTING.md](CONTRIBUTING.md)、脆弱性の報告は [SECURITY.md](SECURITY.md) を参照してください。
