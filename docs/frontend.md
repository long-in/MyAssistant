# フロントエンド

Next.js 16(App Router、`output: "export"`)の静的サイト。Tauri の WebView 内で動き、バックエンドとは `src/lib/api.ts` 経由で通信する([architecture.md](architecture.md))。UI は shadcn/ui + Tailwind CSS 4 で、HUD(ヘッドアップディスプレイ)風のデザイン。

## 画面

| パス | 画面 | 内容 |
| --- | --- | --- |
| `/` | 対話 | 音声入力・応答・字幕・実行ログ・承認 |
| `/knowledge` | ナレッジ | ノートの閲覧、整理、入出力([knowledge.md](knowledge.md)) |
| `/knowledge/all` | 全エージェントのナレッジ | 切り替えずに全エージェントのノートを操作、Finderで開く([knowledge.md](knowledge.md)) |
| `/skills` | スキル | 決まった依頼への対応手順の一覧・編集、選定モデル([skills.md](skills.md)) |
| `/setup` | 設定 | API キー、各種設定([data-and-settings.md](data-and-settings.md)) |
| `/shortcuts` | ショートカット | 一覧。定義は `lib/shortcuts.ts` と共有 |
| `/debug` | デバッグ | デバッグモードが有効なときだけ([debug-log.md](debug-log.md)) |

左のナビゲーション(`AppShell`)で移動する。幅の狭いウィンドウではアイコンだけになる。その左に、パーソナルと各エージェントを並べたエージェントレール(`AgentRail`)がある。メニューの上部には、いまの居場所(パーソナル / エージェント名)を示し、項目は主要と補助に分ける。

## 対話画面(`components/Chat.tsx`)

画面の主な構成:

| 部品 | 役割 |
| --- | --- |
| `VoiceOrb` / `Waveforms` | 円形の波形。マイク入力と読み上げの音量を可視化する |
| `OrbReadout` | 処理段階の表示: 待機中 / 初期化中 / 音声入力中 / 音声解析中 / 思考中 / 音声出力中(段階ごとに色相が変わる) |
| `ExchangeLog` | 直前の 1 往復(ユーザー発話と応答) |
| `ExecutionLog` | コマンド・ファイル操作の実行ログ、計画の進み具合、「中断」ボタン |
| `Subtitles` | 字幕。応答は読み上げの再生位置に同期して表示し、長文は自動でスクロールする。ユーザー発話は手で直して聞き取り訂正として保存できる([voice-input.md](voice-input.md)) |
| `ApprovalModal` | 承認要求(計画・コマンド・許可リストへの追加・ファイル作成)。音声での返答中は聞き取り中の表示が出る |
| `SwitchConfirm` | 会話の切り替えの確認 |
| `ModelSelector` | 使用モデルの選択(キーが未登録のプロバイダーは選べない) |
| `UsageHud` | 当月のトークン使用量と概算金額 |
| `HelpDrawer` | 設定項目ごとの説明 |

- **小さいウィンドウ**では、補助の UI(実行ログ・使用量など)を畳んで音声のオーブを大きく保つ(高さ 40rem 以下または幅 40rem 以下)。
- 「読み上げ / ミュート」の切り替えと、「新しい会話」ボタンがある。
- Whisper のモデルが未ダウンロードのときは、ダウンロードを案内する。

### ワークスペース

- 状態は `lib/workspace.ts` のストア(`useSyncExternalStore`)で持つ。選択中のワークスペース、エージェント一覧、イメージカラーのパレット、設定の読み直しの印(`settingsVersion`)。バックエンドの `workspace-changed` で読み直す。
- 選択中のエージェントの色は、ロゴ、画面上端の帯、対話画面右上のバッジ(`WorkspaceBadge`、名称つき)に出る。CSS 変数 `--workspace-color` にも入れる。
- 切り替えると、`Chat` は `key` で作り直す(読み上げと録音を止める)。切替先でキーの有無を確かめ、未登録なら設定画面へ移る。設定・ナレッジ・デバッグの各画面も `key` で読み直す。
- 会話のセッション ID はワークスペースごとに持つ(`Chat.tsx` の `sessions`)。切り替えて戻ると、同じ会話の続きになる。
- サイドバーを畳んだ状態では、エージェントの色の丸を押して選ぶ(モード切替と合わせて、畳んだままで切り替えられる)。
- 表記: 切り替えの「Personal / Work」とバッジは、ナビゲーションやメニューバー・トレイに合わせて**英語に固定**している。エージェントの作成ダイアログ、設定のコピーの確認、通知のトースト、MFA 入力欄のエージェント表示は、言語設定に従う。

### 状態管理

- 非同期ループ(会話の継続、常時待機、承認の音声待ち)は長く動き続けるため、描画ごとに変わる値は `ref` 経由で最新を読む。
- ループは「生きているか」を返す関数(`alive`)を受け取り、画面の移動・新しい会話・モードの変更で終わる。
- 会話中は `conversing` を立て、常時待機の録音が割り込まないようにする。処理中・承認待ち・読み上げ中も待機録音を止める。
- 詳細: [conversation-session.md](conversation-session.md)、[wake-standby.md](wake-standby.md)。

## キーボードショートカット

`Mod` は macOS では ⌘、それ以外では Ctrl。

| キー | 動作 | 範囲 |
| --- | --- | --- |
| `Space`(押している間) | 押している間録音し、離してから約 0.5 秒(`release_tail_secs`)後に録音を止めて文字起こし・送信する。その間に押し直すと録音を続ける。入力欄にフォーカスがあるときは通常のスペース入力 | 対話画面 |
| `Mod`+`Shift`+`M` | 録音の開始 / 停止 | 対話画面 |
| `Mod`+`1` / `2` / `3` | 対話 / ナレッジ / 設定を開く | どの画面でも |
| `Mod`+`K` | エージェントの検索パレットを開く(名称で絞り込み、↑↓で選び、Enterでそのエージェントのチャットへ切り替え、Escで閉じる) | どの画面でも |
| `Mod`+`/` | ショートカット一覧を開く | どの画面でも |

- 判定は `lib/shortcuts.ts` の `matchShortcut`。IME の変換中(`isComposing`)や、入力欄・セレクトボックスへのフォーカス中は `Space` を奪わない。
- ボタンのクリックは従来どおり開始 / 停止の切り替え。

## エラーと通知

- バックエンドのエラーは `{ code, message, source? }`。`reportError` が表示用の文言にし、タイムアウト・レート制限・サーバーエラー・ネットワークエラー・認証失敗(`AUDIBLE_ERRORS`)では通知音を鳴らす。
- 通知は `sonner` のトースト。

## 設定画面の構成

タブで分け、タブの中の各節は `Heading`(タイトル + ヘルプ)で区切る。タブ名は中身を表す名前にし、性質の違う設定を1つのタブに混ぜない。

### エージェントの設定(`/setup`)

| タブ | 節 |
| --- | --- |
| LLM | AI Link(API キー、AWS、端末内の LLM。リーダーは API キーのコピー)、Limits(検索回数などの上限。リーダー以外)、AI Settings(機能ごとのモデルとプロンプト)、Agent Data(リーダーは Leader Prompt・上限) |
| 音声 | Voice Input(Whisper の取得バージョンを含む)、Voice Output(VOICEVOX の取得バージョンを含む) |
| 権限 | Web Search、Command Tasks、Command Policy(リーダーには無い) |
| Sandbox | リーダー以外。全体設定の有効/無効の表示(切り替えは全体設定。無効でも準備できる)、コンテナの状態・作り直し、`/workspace` の場所と「Finder で開く」、Claude Code の接続先・モデル・上限時間・許可するツール(チェックボックス。一覧にないパターン指定は「その他」欄)、専用の Dockerfile、`sandbox.env` のエディタ([sandbox.md](sandbox.md)) |
| その他 | 名前・色の編集、設定の書き出し・取り込み(Backup)、設定のコピー、Debug Mode、削除 |

### 全体設定(`/global`)

| タブ | 節 |
| --- | --- |
| AIへの指示 | Base Prompt、Shared Data、Personal Data、Knowledge Interval |
| 音声 | Wake Words、Voice Timing |
| Sandbox | 有効/無効のスイッチ、共通の Dockerfile(Sandbox Image) |
| 一般 | Language、OS Notifications、Work(ワークの有効化)、Proxy |
| バックアップ | App Backup(アプリ全体) |

詳細は [global-settings.md](global-settings.md)。

キーの登録時は、疎通確認に成功したときだけ保存する。

## メニューバー常駐(`src-tauri/src/tray.rs`)

- メニューバー(トレイ)にアイコンを常駐させる。アイコンは `icons/tray.png`(黒+透過のテンプレート画像。macOS がメニューバーの配色に合わせて描画する)。
- ウィンドウの「閉じる」操作は**終了ではなく隠す**ことに差し替える。macOS では Dock アイコンも消してメニューバーだけにする(`ActivationPolicy::Accessory`)。
- 左クリックでウィンドウを前面に出す(Dock アイコンも復帰)。右クリックでメニュー: 「MyAssistant を開く」「終了」。
- トレイメニューの先頭に、いまのワークスペース(`Workspace: <名称>`)を表示する。メニューバーとトレイには「Agents」があり、Personal と各エージェントを並べ(選択中にチェック)、選ぶとそのワークスペースへ切り替わる。切り替え・エージェントの追加・名称変更・削除のたびに作り直す(`tray::refresh`)。
- 終了はトレイメニューの「終了」から。終了前に未処理ログを整理する([architecture.md](architecture.md#起動と終了))。
