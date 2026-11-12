# フロントエンド

Next.js 16(App Router、`output: "export"`)の静的サイト。Tauri の WebView 内で動き、バックエンドとは `src/lib/api.ts` 経由で通信する([architecture.md](architecture.md))。UI は shadcn/ui + Tailwind CSS 4 で、HUD(ヘッドアップディスプレイ)風のデザイン。

## 画面

| パス | 画面 | 内容 |
| --- | --- | --- |
| `/` | 対話 | 音声入力・応答・字幕・実行ログ・承認 |
| `/knowledge` | ナレッジ | ノートの閲覧、整理、入出力([knowledge.md](knowledge.md)) |
| `/knowledge/all` | 全エージェントのナレッジ | 切り替えずに全エージェントのノートを操作、Finderで開く([knowledge.md](knowledge.md)) |
| `/skills` | スキル | 決まった依頼への対応手順の一覧・編集、選定モデル([skills.md](skills.md)) |
| `/setup` | 設定 | 選択中のエージェントの設定(下記。[data-and-settings.md](data-and-settings.md)) |
| `/global` | 全体設定 | 全エージェント共通の設定(下記。[global-settings.md](global-settings.md))。`/global#work` で「その他」タブを開く |
| `/shortcuts` | ショートカット | 一覧。定義は `lib/shortcuts.ts` と共有 |
| `/debug` | デバッグ | デバッグモードが有効なときだけ([debug-log.md](debug-log.md)) |

左のナビゲーション(`AppShell`)で移動する。幅の狭いウィンドウではアイコンだけになる。その左に、パーソナルと各エージェントを並べたエージェントレール(`AgentRail`)がある。メニューの上部には、いまの居場所(パーソナル / エージェント名)を示し、項目は主要と補助に分ける。

## 対話画面(`components/Chat.tsx`)

左に会話、右に音声の円形波形と情報カードを並べる。会話は、コーディング支援のチャット(VS Code の Claude Code 拡張)に倣った並び(発話・実行ログ・返答を1列に)にしている。

- **右のパネル**: 上に円形波形(`VoiceOrb` + `OrbReadout`。パネルの幅いっぱいの正方形)、下に情報カード(`InfoCard`: 東京の時刻と日付・世界時計・往復数・モデル・コンテナの起動/停止)。リーダーは情報カードの代わりに台帳(`LeaderLedger`)。幅は固定(18〜20rem。台帳は 30%)で、会話を広く使う。
- **パネルの開閉**: 会話の見出しの右のボタンで畳める(`localStorage` の `chat-side-panel` に覚える)。畳むと会話が全幅になり、入力欄の下の帯に、小さな円形波形・処理段階・往復数・コンテナ・時刻を出す。
- **狭いウィンドウ**(幅 48rem 未満)では、円形波形を会話の上に小さく出す。**小さいウィンドウ**(高さ 40rem 以下または幅 40rem 以下)では、会話を畳んで円形波形を大きく保つ。

| 部品 | 役割 |
| --- | --- |
| `ChatThread` | 会話のスレッド。発話(枠つき)、実行ログ、返答(丸印つき)の順に並べ、最新を追いかける。返答と発話にはコピーボタン、返答には読み上げ・かかった時間・モデル名を添える。送信に失敗した発話は「再送」できる。ユーザー発話は手で直して聞き取り訂正として保存できる([voice-input.md](voice-input.md)) |
| `ChatMarkdown` | Markdown の描画(表・リスト・コード・見出し)。コードブロックは言語名とコピーボタンつき。` ```mermaid ` のブロックは図として描き、図とコードを切り替えられる。画像は読み込まず、リンクは開かない([security.md](security.md))。ナレッジ・スキルの表示でも使う |
| `Reply` | 新しい返答の表示。読み上げ中は再生位置に合わせて字幕のように出し、読み上げないときはタイプライターで出す |
| `TurnLog` / `ExecutionLog` | 発話ごとの実行ログ。`操作名(対象)` の行に、出力を「⎿」でぶら下げる。計画があれば手順ごとにまとめ、実行中は「中断」できる |
| `ChatComposer` | 入力カード。1 つの枠に、入力欄・音声入力・常時待機・読み上げ/ミュート・新しい会話・モデルの切り替え(`ModelPicker`)・送信/中断をまとめる。「/」でスキルを指定する([skills.md](skills.md))。右のパネルを畳んでいるあいだは、枠の下の帯に、処理段階(小さな `VoiceOrb` つき)・往復数・コンテナの状態と起動/停止・時刻(東京)・バージョンを出す |
| `VoiceOrb` / `OrbReadout` | 円形の波形(マイク入力と読み上げの音量)と、その中心の処理段階の表示: 待機中 / 初期化中 / 音声入力中 / 音声解析中 / 思考中 / 音声出力中(段階ごとに色相が変わる) |
| `InfoCard` | 時刻・世界時計・往復数・モデル・コンテナの状態と操作 |
| `ApprovalModal` | 承認要求(計画・コマンド・許可リストへの追加・ファイル作成・資料の上書き・スキルの変更)。音声での返答中は聞き取り中の表示が出る。コマンドの個別承認と許可リストへの追加はボタンのみ([agent-command-tasks.md](agent-command-tasks.md#承認の種類))。Tab の移動はダイアログの中に留める |
| `SwitchConfirm` | 会話の切り替えの確認 |
| `LeaderLedger` | リーダーの台帳。リーダーのときだけ、右のパネルの円形波形の下に並べる |
| `ChatNotices` | 設定不足(キー・モデル・コンテナ)の案内 |

- サンドボックス(コンテナ)の状態と操作は `lib/useSandbox.ts` のフックにまとめている。
- Whisper のモデルが未ダウンロードのときは、ダウンロードを案内する。

### ワークスペース

- 状態は `lib/workspace.ts` のストア(`useSyncExternalStore`)で持つ。選択中のワークスペース、エージェント一覧、イメージカラーのパレット、設定の読み直しの印(`settingsVersion`)。バックエンドの `workspace-changed` で読み直す。
- 選択中のエージェントの色は、ロゴ、画面上端の帯、選択中のメニュー項目に出る(CSS 変数 `--workspace-color`)。
- 切り替えると、`Chat` は `key` で作り直す(読み上げと録音を止める)。切替先でキーの有無を確かめ、未登録なら設定画面へ移る。設定・ナレッジ・デバッグの各画面も `key` で読み直す。
- 会話のセッション ID はワークスペースごとに DB に保存し、開くたびに `chat_session_load` で読む([conversation-session.md](conversation-session.md))。切り替えて戻ると、同じ会話の続きになる。
- エージェントの切り替えは、左端のエージェントレール(`AgentRail`。メニューを畳んでも残る)、`Mod`+`K` のパレット(`AgentPalette`)、メニューバー・トレイから行う([workspace.md](workspace.md#画面))。
- 表記: 切り替えの「Personal / Work」とメニューの項目は、メニューバー・トレイに合わせて**英語に固定**している。エージェントの作成ダイアログ、設定のコピーの確認、通知のトースト、MFA 入力欄のエージェント表示は、言語設定に従う。

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
| `Esc` | 強制中断(依頼の実行・読み上げ・録音・文字起こしの結果・確認待ちをすべて止める。会話の文脈は残る) | 対話画面 |
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
| LLM | AI Link(接続方式(パーソナル・リーダー)、API キー、AWS、端末内の LLM)、Copy API key(リーダーのみ)、AI Limits(検索回数などの上限。リーダー以外)、AI Settings(機能ごとのモデルとプロンプト。[llm-settings.md](llm-settings.md))、Agent Data(ワークのエージェント)/ Leader Prompt・Active Agents(リーダー) |
| 音声 | Voice Input(Whisper の取得バージョンを含む)、Voice Output(VOICEVOX の取得バージョン・読み上げ辞書・定型音声を含む) |
| 権限 | Web Search、Command Policy(リーダーには無い。Command Policy は Bedrock 方式(ワークは常に)でも出さない) |
| Sandbox | 常に出るタブ。ただしパーソナル・リーダーは、AI Link の接続方式が API のあいだは、「Bedrock 方式のときだけ使えます」という案内だけを出す(接続方式の読み込みが終わるまでは何も出さない)。ワークのエージェントは常にサンドボックスで動き、切り替えは無い。準備のチェックリスト(Docker・イメージのビルド・コンテナの起動。`SandboxSteps`)と、詳細設定(担当システムのフォルダ `/knowledge/data`、専用の Dockerfile、`sandbox.env`、上限時間と自動停止、Claude Code の settings.json)([sandbox.md](sandbox.md#設定画面)) |
| その他 | 名前・色の編集(Agent)、使うプロキシの選択(Proxy)、設定の書き出し・取り込み(Backup)、設定のコピー(Copy Settings。ワークのエージェント)、Debug Mode、削除(Danger Zone。ワークのエージェント) |

### 全体設定(`/global`)

| タブ | 節 |
| --- | --- |
| AIへの指示 | Base Prompt、Shared Prompts(チャット以外のプロンプト)、Shared Data、Personal Data |
| 音声 | Wake Words、Voice Timing |
| Sandbox | 共通の Dockerfile(Sandbox Image)、Shared Claude Settings(全体の settings.json) |
| その他 | Language、OS Notifications、Work(ワークの有効化)、Proxy、App Backup |

詳細は [global-settings.md](global-settings.md)。

キーの登録時は、疎通確認に成功したときだけ保存する。

## メニューバー常駐(`src-tauri/src/tray.rs`)

- メニューバー(トレイ)にアイコンを常駐させる。アイコンは `icons/tray.png`(黒+透過のテンプレート画像。macOS がメニューバーの配色に合わせて描画する)。
- ウィンドウの「閉じる」操作は**終了ではなく隠す**ことに差し替える。macOS では Dock アイコンも消してメニューバーだけにする(`ActivationPolicy::Accessory`)。
- 左クリックでウィンドウを前面に出す(Dock アイコンも復帰)。右クリックでメニュー: 「MyAssistant を開く」「終了」。
- トレイメニューの先頭に、いまのワークスペース(`Workspace: <名称>`)を表示する。メニューバーとトレイには「Agents」があり、Personal と各エージェントを並べ(選択中にチェック)、選ぶとそのワークスペースへ切り替わる。切り替え・エージェントの追加・名称変更・削除のたびに作り直す(`tray::refresh`)。
- 終了はトレイメニューの「終了」から。終了前に、動いているサンドボックスのコンテナを止める([architecture.md](architecture.md#起動と終了))。
