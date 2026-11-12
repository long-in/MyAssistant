# MyAssistant 設計書

機能ごとの詳細設計です。プロジェクトの概要と使い方は [../README.md](../README.md) を参照してください。

### 全体

| 文書 | 内容 |
| --- | --- |
| [architecture.md](architecture.md) | 全体構成、プロセス間の通信、管理状態、起動・終了の流れ |
| [frontend.md](frontend.md) | 画面構成、設定画面のタブ、ショートカット、メニューバー常駐 |
| [data-and-settings.md](data-and-settings.md) | 保存データ、SQLite のスキーマ、設定項目、調整値、設定のエクスポート・インポート |
| [security.md](security.md) | API キー・AWS 認証の扱い、パスのサンドボックス、プロンプトインジェクション対策、送信先 |

### 対話と AI

| 文書 | 内容 |
| --- | --- |
| [chat-flow.md](chat-flow.md) | 対話(`send_chat`)の処理(ホストとコンテナの2経路)、システムプロンプトの組み立て、会話の要約 |
| [llm-settings.md](llm-settings.md) | AI の接続方式(API / Bedrock)、モデルのカタログ、機能ごとのモデル、プロンプトの上書き |
| [local-llm.md](local-llm.md) | ローカル LLM(Qwen 2.5 7B)の取得、llama-server の起動・停止、形式の変換 |
| [conversation-session.md](conversation-session.md) | 会話の継続と切り替え、履歴からの再開、自動で会話を続けるループ |
| [agent-file-tools.md](agent-file-tools.md) | ホストのツールループ、ファイル操作・設計書の参照・インターネット検索のツール |
| [agent-command-tasks.md](agent-command-tasks.md) | 作業依頼(計画の承認とコマンド実行) |
| [command-policy.md](command-policy.md) | コマンドの許可・拒否の判定とポリシー(DB) |
| [sandbox.md](sandbox.md) | Docker サンドボックス(コンテナ内の Claude Code の実行、認証情報、マウント、承認、Dockerfile、コンテナの準備と後始末、後回しにした項目) |
| [skills.md](skills.md) | スキル(決まった依頼への対応手順)、軽量モデルによる選定、会話からの管理 |

### 音声

| 文書 | 内容 |
| --- | --- |
| [voice-input.md](voice-input.md) | 録音、発話判定(VAD)、音声認識(Whisper / Google)、校正、聞き取り訂正 |
| [voice-output.md](voice-output.md) | 音声合成(TTS)、読み上げ辞書、字幕の同期、定型音声 |
| [wake-standby.md](wake-standby.md) | 常時音声待機と起動ワード |

### ナレッジ

| 文書 | 内容 |
| --- | --- |
| [knowledge.md](knowledge.md) | ナレッジの保存形式、キャプチャ、整理バッチ、学びの提案と棚卸し、入出力 |
| [knowledge-graph.md](knowledge-graph.md) | グラフ索引と関連ノートの想起 |
| [memory.md](memory.md) | 記憶(覚えておくこと) |

### ワークスペースと設定

| 文書 | 内容 |
| --- | --- |
| [workspace.md](workspace.md) | パーソナルとワークのエージェント、保存場所、切り替え、依頼の継続、通知、設定のコピー、未対応事項 |
| [leader.md](leader.md) | リーダー(ワークの有効化、指示の配布、台帳、宛先の提案) |
| [global-settings.md](global-settings.md) | 全エージェント共通の全体設定 |
| [user-profile.md](user-profile.md) | パーソナルデータ |
| [proxy.md](proxy.md) | 通信のプロキシ(登録・既定・エージェントごとの選択) |
| [backup.md](backup.md) | アプリ全体のバックアップとリストア(zip の中身、検証、再起動時の差し替え) |
| [debug-log.md](debug-log.md) | デバッグモードとログ画面 |

## 書き方の方針

- ソースの該当箇所をモジュール名(例: `services/graph.rs`)で示します。数値の上限・既定値はコード上の定数と一致させています。変更したときはこの文書も更新してください。
- 設計書はバイナリへ埋め込み、チャットの設計書ツールから読みます。文書を追加・削除したら `src-tauri/src/services/docs.rs` の `DOCS` も直します(`embedded_docs_match_docs_dir` が漏れを検知します)。
- 「決定事項」は意図して選んだ仕様、「既知の限界」は仕様として受け入れている制約です。
