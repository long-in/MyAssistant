# 全体設定

すべてのエージェント(パーソナルを含む)で共通の項目を、1か所で管理する機能です。実装は `services/global_settings.rs`、`commands/global_settings.rs`、画面は左メニューの「Global Settings」(`/global`)。

## 方針

- 全体設定の内容は、すべてのエージェントに反映される。
- **各エージェントから上書きできない。** エージェントのDBに同名の設定が残っていても読まない。全体設定のファイルができたあと(初回の取り込みのあと)は、DBを開いたときに古い値を消す(`purge_legacy`)。ファイルが無いうちは、取り込みの元になるので消さない。
- 保存先の取り違えを防ぐため、DBへの全体設定のキーの書き込みと、全体設定へのエージェントごとのキーの書き込みは拒否する(キーの一覧は `services/settings_registry.rs`)。
- 保存先はアプリデータの `workspaces/global_settings.json`(キーと文字列値の対応)。共通データと同じく、エージェントを削除しても残る。書き込みは一時ファイルからのリネームで、壊れていたら `.corrupt` へ退避して空から始める。

## 画面の構成(タブ)

- **AIへの指示**: ベースプロンプト、共通データ、パーソナルデータ、ナレッジ整理の間隔。
- **音声**: 起動ワード、音声入力の待ち時間。
- **Sandbox**: サンドボックスの有効/無効と、共通の Dockerfile([sandbox.md](sandbox.md))。
- **その他**: 表示言語、OS通知、ワークの有効化・無効化(`work_enabled`。バックアップ・設定のコピーの対象外。[workspace.md](workspace.md))、通信のプロキシ([proxy.md](proxy.md))、全体設定の書き出し・取り込みと、アプリ全体のバックアップ([backup.md](backup.md))。`/global#work` で、このタブを開く。

## 項目

| 項目 | 画面 | キー | 備考 |
| --- | --- | --- | --- |
| ベースプロンプト | Base Prompt | `base_instructions` | 空は既定文 |
| OS通知 | OS Notifications | `os_notifications` | 旧 `localStorage` の値は初回に取り込む |
| 表示言語 | Language | `language` | `ja` / `en`。音声認識の言語(`stt_language`)はエージェントごとのまま |
| 音声入力の待ち時間 | Voice Timing | `voice_timing` | `voice_idle_secs` / `utterance_silence_secs` / `release_tail_secs`。Limits(検索回数など)はエージェントごと |
| 起動ワード | Wake Words | `wake_enabled` / `wake_words` | |
| 共通データ | Shared Data | (`workspaces/shared_data.json`) | 全エージェントで1つ。**渡す先はエージェントのみ**。パーソナルには渡さない([workspace.md](workspace.md)) |
| パーソナルデータ | Personal Data | `user_profile` | **渡す先は従来どおりパーソナルのみ**。エージェントには渡さない([user-profile.md](user-profile.md)) |
| ナレッジ整理の間隔 | Knowledge Interval | `batch_interval_minutes` | 定期バッチは、全エージェント同じ間隔で動く |

## 初回の取り込み

起動時に `global_settings.json` が無ければ、**パーソナルのDBの現在値**を初期値として写す(`migrate_from`)。VOICE TIMING は、パーソナルの `tuning` から該当の3項目を取り出す。以後は取り込まない。

表示言語とOS通知は端末の `localStorage` にも値があるため、画面が起動時に `get_global_prefs` で確かめ、全体設定が未保存なら、この端末の値を保存する(`lib/globalSync.ts`)。`localStorage` には、起動時の表示のちらつきを避けるための写しだけを残す。

## 設定のコピー・バックアップとの関係

- エージェント間の設定のコピーとエクスポート/インポートは、全体設定の項目を扱わない。
- 全体設定だけを書き出す機能は無い。全体設定は App Backup(アプリ全体)に含まれる([backup.md](backup.md))。

## 注意

- Wake Words の動作確認(定型音声・音声認識エンジン)は、いま開いているエージェントのAPIキー・音声の状態を使う。設定の値そのものは全エージェント共通。
