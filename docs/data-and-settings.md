# 保存データと設定

## 保存場所

すべてアプリデータディレクトリ(識別子 `dev.longin.tachicoma-agent`)に保存され、リポジトリには含まれない。macOS では `~/Library/Application Support/dev.longin.tachicoma-agent/`。

| パス | 内容 |
| --- | --- |
| `timeline.db` | SQLite。会話ログ・使用量・アプリ設定 |
| `secrets.json` | API キー。**平文 JSON**(権限 0600)。OS のキーチェーンは使っていない([security.md](security.md)) |
| `models/` | Whisper モデル(`ggml-large-v3-turbo-q5_0.bin` / `ggml-small.bin`) |
| `voice/` | 定型音声(`<id>-<ハッシュ>.mp3`) |
| ナレッジ | 既定ではアプリデータ配下。設定で任意のフォルダへ変更できる([knowledge.md](knowledge.md)) |

識別子は、保存先を変えないためあえて旧名のまま。`.tachicoma/`(コマンド権限)や設定ファイルの内部 ID(`tachicoma-agent`)、エクスポートのフォルダ名も同じ理由で変えていない。

## SQLite スキーマ(`db/mod.rs`)

```mermaid
erDiagram
  conversation_logs {
    TEXT id PK
    TEXT session_id
    TEXT role "user | assistant"
    TEXT content
    TEXT provider
    TEXT model
    DATETIME created_at
    BOOLEAN is_processed
  }
  usage_logs {
    INTEGER id PK
    TEXT provider
    TEXT model
    INTEGER input_tokens
    INTEGER output_tokens
    DATETIME created_at
  }
  app_settings {
    TEXT key PK
    TEXT value
    DATETIME updated_at
  }
```

| テーブル | 役割 | 主な操作 |
| --- | --- | --- |
| `conversation_logs` | 会話の一次ログ(時系列)。インデックスは `(session_id, created_at)` と `is_processed` | 保存、直近 N 件・全件の取得、未処理の取得、処理済みにする |
| `usage_logs` | LLM 呼び出しごとのトークン使用量 | 記録、当月(ローカル時間)のプロバイダー・モデル別集計 |
| `app_settings` | キー・値の設定(値は文字列。構造のあるものは JSON) | 取得、upsert |

- `conversation_logs.is_processed`: 整理バッチがナレッジへ反映したかどうか([knowledge.md](knowledge.md))。
- 接続は `Mutex<Connection>`。ロックが毒されても(パニック後)中身を使い続ける。

## 設定項目(`app_settings`)

| キー | 値 | 既定 | 設定画面 |
| --- | --- | --- | --- |
| `routing` | `Route`(JSON) | Claude が使えれば Sonnet | 対話画面のモデル選択 |
| `base_instructions` | 文字列 | 既定文 | Base Prompt |
| `web_access` | `0` / `1` | 有効 | Web Search |
| `shell_access` | `0` / `1` | 有効 | Command Tasks |
| `stt_engine` | `local` / `google` / `aqua` | `local` | Voice Link |
| `stt_language` | `auto` / `ja` / `en` / `zh` / `ko` | `auto` | Voice Link |
| `proofread_route` | `Route`(JSON)。空ならオフ | オフ | Proofread |
| `wake_enabled` / `wake_words` | `0` / `1`、JSON 配列 | 無効 | Wake Words |
| `tuning` | 調整項目(JSON) | 下表 | Voice Timing / Limits |
| `batch_route` | `Route`(JSON) | 既定 | ナレッジ画面 |
| `batch_interval_minutes` | 5〜1,440 | 30 | Knowledge Interval |
| `debug_mode` | `0` / `1` | 無効 | Debug Mode |
| `user_profile` | パーソナルデータ(JSON) | 有効・全項目未設定 | Personal Data |
| `knowledge_dir` | 絶対パス(空なら既定) | 既定 | ナレッジ画面 |
| `summary:<session_id>` | 会話の要約(JSON) | — | — |

自動保存: 調整項目と Command Policy は、入力欄を離れたときに保存する。

## 調整項目(`tuning.rs`)

数値は保存時に範囲内へ丸める。未保存・欠けた項目は既定値。非数は下限になる。

| 項目 | 意味 | 範囲 | 既定 |
| --- | --- | --- | --- |
| `web_search_max` | 1 回の依頼で行うインターネット検索の上限回数 | 1〜20 | 5 |
| `web_fetch_max` | 1 回の依頼で行うページ取得の上限回数 | 1〜20 | 5 |
| `voice_idle_secs` | 声が入らなければ音声入力を終える時間(秒) | 1.0〜30.0 | 3.0 |
| `utterance_silence_secs` | 話し終わりとみなす無音の長さ(秒) | 0.5〜5.0 | 1.2 |
| `release_tail_secs` | スペースを離してから録音を止めるまでの猶予(秒) | 0.0〜3.0 | 0.5 |
| `history_exchanges` | 過去の会話をそのまま LLM へ渡す往復数の上限。超えた分は要約 | 2〜30 | 10 |

## 使用量の表示

- `usage_logs` を当月(ローカル時間)で集計し、プロバイダー・モデル別に表示する(`UsageHud`)。
- 金額は `src/lib/pricing.ts` の単価(100 万トークンあたり、USD)からの**概算**で、請求額とは一致しない場合がある。単価が未登録のモデルは金額を出さない。
- このアプリ経由の呼び出しだけのローカル集計。Web 検索の追加料金や、Google Cloud・Aqua Voice の利用料は含まない。

## 設定のエクスポート・インポート(`settings_transfer.rs`)

別の端末への移行やバックアップに使う。設定画面の Backup。

### 対象

- `app_settings` のうち上表の**設定項目**(`routing` から `user_profile` まで。パーソナルデータを含む)と、**コマンド権限**(`command-policy.json`)。
- **含まない**: API キー、端末ごとに異なるナレッジの保存先(`knowledge_dir`)、会話ごとの要約、会話の履歴。

### ファイル

```json
{
  "app": "tachicoma-agent",
  "version": 1,
  "exported_at": "2026-10-29T03:00:00Z",
  "settings": { "routing": "{...}", "shell_access": "1" },
  "command_policy": { "version": 3, "allow": [] }
}
```

- 保存先は、ダイアログで選んだ拡張子 `.json` の絶対パスのみ。既定のファイル名は `myassistant-settings-<日時>.json`。
- 未設定の項目は書き出さない。

### 取り込み

1. 1,000,000 バイトを超えるファイルは拒否する。
2. `app` と `version` を確認する(別アプリのファイル・未対応の版は拒否)。
3. **全項目を検証してから書き込む**。型ごとに検証する: フラグ(`0` / `1`)、JSON として読めるか、許可された値のどれか、文字列(20,000 文字以内)、パーソナルデータ(通常の入力と同じ検証)。**不正な項目が 1 つでもあれば何も変更しない**。
4. ファイルに無い項目は変更しない。未知の項目は読み飛ばす(`skipped`)。
5. コマンド権限は検証(`cleaned`)して保存する。デバッグモードの出力先を決め直す。

結果は `{ applied, skipped }` で返す。
