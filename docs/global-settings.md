# 全体設定

すべてのスペース(パーソナルを含む)で共通の項目を、1か所で管理する機能です。実装は `services/global_settings.rs`、`commands/global_settings.rs`、画面は左メニューの「Global Settings」(`/global`)。

## 方針

- 全体設定の内容は、すべてのスペースに反映される。
- **各スペースから上書きできない。** スペースのDBに同名の設定が残っていても読まない(消さずに無視する)。
- 保存先はアプリデータの `workspaces/global_settings.json`(キーと文字列値の対応)。共通データと同じく、スペースを削除しても残る。書き込みは一時ファイルからのリネームで、壊れていたら `.corrupt` へ退避して空から始める。

## 画面の構成(タブ)

- **AIの動作**: ベースプロンプト、パーソナルデータ。
- **音声**: 起動ワード、音声入力の待ち時間。
- **アプリ**: ナレッジ整理の間隔、表示言語、OS通知、全体設定のバックアップ。
- **ワーク**: ワークの有効化・無効化(`work_enabled`。バックアップ・設定のコピーの対象外。[workspace.md](workspace.md))。`/global#work` で、このタブを開く。

## 項目

| 項目 | 画面 | キー | 備考 |
| --- | --- | --- | --- |
| ベースプロンプト | Base Prompt | `base_instructions` | 空は既定文 |
| OS通知 | OS Notifications | `os_notifications` | 旧 `localStorage` の値は初回に取り込む |
| 表示言語 | Language | `language` | `ja` / `en`。音声認識の言語(`stt_language`)はスペースごとのまま |
| 音声入力の待ち時間 | Voice Timing | `voice_timing` | `voice_idle_secs` / `utterance_silence_secs` / `release_tail_secs`。Limits(検索回数など)はスペースごと |
| 起動ワード | Wake Words | `wake_enabled` / `wake_words` | |
| パーソナルデータ | Personal Data | `user_profile` | **渡す先は従来どおりパーソナルのみ**。スペースには渡さない([user-profile.md](user-profile.md)) |
| ナレッジ整理の間隔 | Knowledge Interval | `batch_interval_minutes` | 定期バッチは、全スペース同じ間隔で動く |

## 初回の取り込み

起動時に `global_settings.json` が無ければ、**パーソナルのDBの現在値**を初期値として写す(`migrate_from`)。VOICE TIMING は、パーソナルの `tuning` から該当の3項目を取り出す。以後は取り込まない。

表示言語とOS通知は端末の `localStorage` にも値があるため、画面が起動時に `get_global_prefs` で確かめ、全体設定が未保存なら、この端末の値を保存する(`lib/globalSync.ts`)。`localStorage` には、起動時の表示のちらつきを避けるための写しだけを残す。

## 設定のコピー・バックアップとの関係

- スペース間の設定のコピーとエクスポート/インポートは、全体設定の項目を扱わない。
- 全体設定だけのエクスポート/インポートは、Global Settings の Backup(`export_global_settings` / `import_global_settings`)。全項目を検証してから書き込み、不正な項目があれば何も変更しない。スペースごとの設定やAPIキーは含まない。

## 注意

- Wake Words の動作確認(定型音声・音声認識エンジン)は、いま開いているスペースのAPIキー・音声の状態を使う。設定の値そのものは全スペース共通。
