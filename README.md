# MyAssistant

音声で対話できるデスクトップ AI エージェントです。

- 会話はナレッジ(Markdown)として蓄積され、次の会話で関連する内容が自動で想起されます。
- ナレッジの閲覧・作成、インターネット検索、コマンドによる作業も、声で依頼できます。

| 項目 | 内容 |
| --- | --- |
| フロントエンド | Next.js 16 (静的エクスポート) / React 19 / Tailwind CSS 4 / shadcn/ui |
| バックエンド | Tauri 2 (Rust) |
| 対応 OS | **macOS 専用(11.0 以上)** |

Windows / Linux は未対応です(コマンド実行が `/bin/sh` 前提、音声認識が Metal 前提のため)。

機能ごとの詳細設計は [docs/](docs/README.md) を参照してください。

## 目次

- [機能](#機能)
- [必要環境](#必要環境)
- [開発](#開発)
- [ビルド](#ビルド)
- [初期設定](#初期設定)
- [データの保存場所](#データの保存場所)
- [ディレクトリ構成](#ディレクトリ構成)
- [セキュリティ](#セキュリティ)
- [テスト](#テスト)
- [ライセンス](#ライセンス)

## 機能

### 対話

- **LLM 対話** ([chat-flow.md](docs/chat-flow.md))
  - Anthropic / OpenAI に対応し、使うモデルは画面で選びます。
  - 応答は読み上げ向けの短い話し言葉です。
  - 読み上げ後は自動で音声入力が再開し、会話を続けられます。
- **会話の継続と切り替え** ([conversation-session.md](docs/conversation-session.md))
  - 切り替えを明示しない限り、会話は継続します(放置しても切り替わりません)。
  - 切り替わるのは、「新しい会話」ボタンを押したとき、または「話題を変えて」などの発話に「はい」と答えたときだけです。
  - 長くなった会話は自動で要約し、毎回送るトークンを抑えます。
- **パーソナルデータ(オプション)** ([user-profile.md](docs/user-profile.md))
  - 名前・呼ばれ方・年齢・居住地などを手動で登録すると、呼びかけや回答の調整に使います。
  - 会話からの自動抽出はしません。

### 音声

- **音声入力 (STT)** ([voice-input.md](docs/voice-input.md))
  - 次から選べます。
    - ローカルの Whisper(既定。初回にモデルをダウンロード)
    - Google Cloud Speech-to-Text
    - Aqua Voice
  - Whisper のモデルは `ggml-large-v3-turbo-q5_0.bin`(約574MB)または `ggml-small.bin`(約466MB)です。
  - Silero VAD で声のない入力を除き、無音への幻聴を防ぎます。
- **文字起こしの校正(オプション)**
  - 設定でモデルを選ぶと、会話の文脈と訂正履歴から聞き間違いを直してから送信します。
  - 字幕から聞き取りを手で訂正すると、ナレッジに記録されて次の発話から反映されます。
- **音声出力 (TTS)** ([voice-output.md](docs/voice-output.md))
  - 次から選べます。
    - Google Cloud Text-to-Speech(日本語 / 英語を自動判別)
    - VOICEVOX CORE(端末内で合成。キー不要・無料。設定画面から約90MBを取得)
  - 字幕は読み上げの再生位置に合わせて表示されます。
  - キーが無ければ字幕だけで応答します。
- **常時音声待機(オプション)** ([wake-standby.md](docs/wake-standby.md))
  - 起動ワードを登録すると、対話画面を開いている間マイクを開いたままにします。
  - 起動ワードを含む発話があったときだけ依頼を受け付け、含まない発話は LLM へ送りません。

### ナレッジ

- **蓄積と整理** ([knowledge.md](docs/knowledge.md))
  - 会話は即座に「キャプチャ」として Markdown に残ります。
  - 未処理のログは、LLM がトピック別のノートへ整理します。
    - 実行タイミング: 設定した間隔(既定30分)、アプリ終了時、「今すぐ整理」
  - 保存先は設定で変更でき、Obsidian の vault としても開けます。
- **関連ノートの想起** ([knowledge-graph.md](docs/knowledge-graph.md))
  - ノートをタグ・リンク・同じ会話由来のつながりでグラフにします。
  - 発話に関連するノートを最大2ホップ辿り、要約をプロンプトへ渡します。
  - LLM やネットワークは使いません。

### 作業の依頼(Claude 選択時のみ)

- **ファイル操作** ([agent-file-tools.md](docs/agent-file-tools.md))
  - ナレッジの一覧・読み取り・検索・新規作成ができます。
  - 作成は承認制で、上書きはできません。
- **インターネット検索**
  - Anthropic のサーバーツールで、検索とページ取得を行います。
  - 検索ごとに追加料金がかかるため、設定でオフにでき、回数の上限も設定できます。
- **コマンド実行** ([agent-command-tasks.md](docs/agent-command-tasks.md) / [command-policy.md](docs/command-policy.md))
  - 「〜してください」という依頼に手順を提示し、音声で「はい」と承認すると実行します。
  - 制限:
    - `rm`・`sudo`・OS に影響するコマンドは常に拒否します。
    - 読み取り系以外のコマンドは、ナレッジのディレクトリ内でしか実行できません。
  - 許可・拒否・都度承認のコマンドは、設定画面の Command Policy で編集できます(実体はナレッジ内の `.tachicoma/command-policy.json`)。

### その他

- **使用量表示:** プロバイダー・モデル別の当月のトークン使用量と概算金額を表示します。
- **メニューバー常駐:** ウィンドウを閉じてもメニューバーに残り、アイコンのクリックで再表示します。終了はメニューから行います([frontend.md](docs/frontend.md))。
- **バックアップ:** 設定(API キーを除く)を JSON でエクスポート・インポートできます([data-and-settings.md](docs/data-and-settings.md))。
- **デバッグモード(オプション):** API の通信と文字起こしをナレッジ配下に記録し、画面で確認できます([debug-log.md](docs/debug-log.md))。

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
| `Space`(押している間) | 録音し、離すと文字起こし・送信(対話画面。入力欄フォーカス時は通常のスペース) |
| `Mod`+`Shift`+`M` | 録音の開始 / 停止 |
| `Mod`+`1` / `2` / `3` | 対話 / ナレッジ / 設定を開く |
| `Mod`+`/` | ショートカット一覧を開く |

## 必要環境

- macOS(必須)
- Node.js と npm
- Rust (stable、1.90 以上)
- Xcode Command Line Tools(whisper.cpp のビルドに C++ コンパイラと CMake が必要)

## 開発

```bash
npm install
npm run tauri dev
```

Next.js のみを起動する場合は `npm run dev` です(Tauri のコマンドは使えません)。

チェックとテスト:

```bash
npm run lint
cd src-tauri && cargo test
```

`cargo` が PATH に無い場合は、`~/.rustup/toolchains/stable-*/bin` を PATH に追加してください。

## ビルド

```bash
npm run tauri build
```

- 成果物は `src-tauri/target/release/bundle/` 配下に出力されます。
- macOS の最小対応バージョンは 11.0 です([src-tauri/tauri.conf.json](src-tauri/tauri.conf.json))。
- whisper.cpp のビルドには macOS 10.15 以上のデプロイターゲットが必要なため、この値を下げないでください。

## 初期設定

### 1. API キーを登録する

アプリの設定画面で登録します。登録時に疎通確認を行い、成功したときだけ保存します。

| 用途 | キー |
| --- | --- |
| Claude | Anthropic API キー |
| GPT | OpenAI API キー |
| 音声出力 / Google STT | Google Cloud API キー |
| Aqua Voice(STT) | Aqua Voice API キー |

### 2. 音声まわりを準備する

- ローカル Whisper を使う場合は、設定画面からモデルをダウンロードしてください。
- 初回の録音時に、マイクの使用許可を求められます。
- 常時音声待機や会話の切り替えの確認で音声を使うには、設定画面の「定型音声を作成」(選んだ読み上げエンジンで合成)を先に実行してください。

## データの保存場所

すべてアプリデータディレクトリに保存され、リポジトリには含まれません。

- 識別子: `dev.longin.tachicoma-agent`
- macOS のパス: `~/Library/Application Support/dev.longin.tachicoma-agent/`
- 識別子は保存先を変えないため、アプリ名の変更後も旧名のままにしています。

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
  lib/                  Tauri 呼び出し、音声での承認・切り替えの判定、
                        字幕の同期、ショートカット、料金計算
src-tauri/              Tauri バックエンド (Rust)
  src/commands/         フロントエンドから呼ぶコマンド
  src/services/         LLM クライアント、STT/TTS、整理バッチ、ナレッジ、
                        エージェント、コマンド判定、キー保存
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

[MIT](LICENSE)

- 同梱モデルの表記: [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)
- 貢献の方法: [CONTRIBUTING.md](CONTRIBUTING.md)
- 脆弱性の報告: [SECURITY.md](SECURITY.md)
