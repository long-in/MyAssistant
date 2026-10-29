# MyAssistant 設計書

機能ごとの詳細設計です。プロジェクトの概要と使い方は [../README.md](../README.md) を参照してください。

| 文書 | 内容 |
| --- | --- |
| [architecture.md](architecture.md) | 全体構成、プロセス間の通信、起動・終了の流れ |
| [chat-flow.md](chat-flow.md) | 対話(`send_chat`)の処理、システムプロンプトの組み立て、モデル選択、会話の要約 |
| [conversation-session.md](conversation-session.md) | 会話の継続と切り替え、自動で会話を続けるループ |
| [voice-input.md](voice-input.md) | 録音、発話判定(VAD)、音声認識(3 エンジン)、校正、聞き取り訂正 |
| [voice-output.md](voice-output.md) | 音声合成(TTS)、字幕の同期、定型音声 |
| [wake-standby.md](wake-standby.md) | 常時音声待機と起動ワード |
| [knowledge.md](knowledge.md) | ナレッジの保存形式、キャプチャ、整理バッチ、入出力 |
| [knowledge-graph.md](knowledge-graph.md) | グラフ索引と関連ノートの想起 |
| [user-profile.md](user-profile.md) | パーソナルデータ |
| [agent-file-tools.md](agent-file-tools.md) | ファイル操作・インターネット検索のツール |
| [agent-command-tasks.md](agent-command-tasks.md) | 作業依頼(計画の承認とコマンド実行) |
| [command-policy.md](command-policy.md) | コマンドの許可・拒否の判定とポリシーファイル |
| [data-and-settings.md](data-and-settings.md) | 保存データ、設定項目、調整値、設定のエクスポート・インポート |
| [security.md](security.md) | API キーの扱い、パスのサンドボックス、プロンプトインジェクション対策 |
| [debug-log.md](debug-log.md) | デバッグモードとログ画面 |
| [frontend.md](frontend.md) | 画面構成、ショートカット、メニューバー常駐 |

## 書き方の方針

- ソースの該当箇所をモジュール名(例: `services/graph.rs`)で示します。数値の上限・既定値はコード上の定数と一致させています。変更したときはこの文書も更新してください。
- 「決定事項」は意図して選んだ仕様、「既知の限界」は仕様として受け入れている制約です。
