# 全体設定

すべてのエージェント(パーソナルを含む)で共通の項目を、1か所で管理する機能です。実装は `services/global_settings.rs`、`commands/global_settings.rs`、画面は左メニューの「Global Settings」(`/global`)。

## 方針

- 全体設定の内容は、すべてのエージェントに反映される。
- **各エージェントから上書きできない。** エージェントのDBに同名の設定が残っていても読まない。
- 保存先の取り違えを防ぐため、DBへの全体設定のキーの書き込みと、全体設定へのエージェントごとのキーの書き込みは拒否する(キーの一覧は `services/settings_registry.rs`)。
- 保存先はアプリデータの `work/global_settings.db`(専用の SQLite。キーと文字列値の対応)。共通データと同じく、エージェントを削除しても残る。複数項目の書き込みは1つのトランザクションで、途中で失敗したらどれも書かれない。以前の `work/global_settings.json` があれば、開くときに一度だけ取り込んで削除する(DB にある項目は上書きしない)。

## 画面の構成(タブ)

- **AIへの指示**: ベースプロンプト(Base Prompt)、チャット以外のプロンプト(Shared Prompts)、共通データ、パーソナルデータ。
- **音声**: 起動ワード、音声入力の待ち時間。
- **Sandbox**: 共通の Dockerfile(Sandbox Image)と、全体の Claude Code の settings.json(Shared Claude Settings)。ワークのエージェントは常にサンドボックスで動くため、有効/無効の切り替えは無い([sandbox.md](sandbox.md))。
- **その他**: 表示言語、OS通知、ワークの有効化・無効化(`work_enabled`。[leader.md](leader.md#ワークの有効化無効化))、通信のプロキシ(複数登録と既定。[proxy.md](proxy.md))、全体設定の書き出し・取り込みと、アプリ全体のバックアップ([backup.md](backup.md))。`/global#work` で、このタブを開く。

## 項目

| 項目 | 画面 | キー | 備考 |
| --- | --- | --- | --- |
| ベースプロンプト | Base Prompt | `base_instructions` | 空は既定文。エージェントの `prompt_chat` が優先 |
| OS通知 | OS Notifications | `os_notifications` | 旧 `localStorage` の値は初回に取り込む |
| 表示言語 | Language | `language` | `ja` / `en`。音声認識の言語(`stt_language`)はエージェントごとのまま |
| 音声入力の待ち時間 | Voice Timing | `voice_timing` | `voice_idle_secs` / `utterance_silence_secs` / `release_tail_secs`。Limits(検索回数など)はエージェントごと |
| 起動ワード | Wake Words | `wake_enabled` / `wake_words` | [wake-standby.md](wake-standby.md) |
| チャット以外のプロンプト | Shared Prompts | `global_prompt_summary` / `_proofread` / `_organize_v2` / `_consolidate` / `_skills` / `_suggest` | エージェントが上書きしていなければ使う([llm-settings.md](llm-settings.md#プロンプト)) |
| Claude Code の settings.json | Shared Claude Settings | `sandbox_claude_settings_shared` | エージェントが保存していなければ使う([sandbox.md](sandbox.md#認証情報)) |
| ワークの有効化 | Work | `work_enabled` | `0` で無効。未保存は有効([leader.md](leader.md)) |
| 通信のプロキシ | Proxy | `proxy_list` / `proxy_default` | パスワードは `proxy/secrets.json`([proxy.md](proxy.md)) |
| 共通データ | Shared Data | (`work/shared_data.json`) | 全エージェントで1つ。**渡す先はエージェントのみ**。パーソナルには渡さない([workspace.md](workspace.md)) |
| パーソナルデータ | Personal Data | `user_profile` | **渡す先は従来どおりパーソナルのみ**。エージェントには渡さない([user-profile.md](user-profile.md)) |

## 表示言語とOS通知

表示言語とOS通知は端末の `localStorage` にも値があるため、画面が起動時に `get_global_prefs` で確かめ、全体設定が未保存なら、この端末の値を保存する(`lib/globalSync.ts`)。`localStorage` には、起動時の表示のちらつきを避けるための写しだけを残す。

## 設定のコピー・バックアップとの関係

- エージェント間の設定のコピーとエクスポート/インポートは、全体設定の項目を扱わない。
- 全体設定だけを書き出す機能は無い。全体設定(`work/global_settings.db` の全項目)は App Backup(アプリ全体)に含まれる([backup.md](backup.md))。共通データ(`work/shared_data.json`)は、いまはバックアップに含まれない(backup.md の既知の限界)。

## 注意

- Wake Words の動作確認(定型音声・音声認識エンジン)は、いま開いているエージェントのAPIキー・音声の状態を使う。設定の値そのものは全エージェント共通。
