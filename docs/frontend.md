# フロントエンド

Next.js 16(App Router、`output: "export"`)の静的サイト。Tauri の WebView 内で動き、バックエンドとは `src/lib/api.ts` 経由で通信する([architecture.md](architecture.md))。UI は shadcn/ui + Tailwind CSS 4 で、HUD(ヘッドアップディスプレイ)風のデザイン。

## 画面

| パス | 画面 | 内容 |
| --- | --- | --- |
| `/` | 対話 | 音声入力・応答・字幕・実行ログ・承認 |
| `/knowledge` | ナレッジ | ノートの閲覧、整理、入出力([knowledge.md](knowledge.md)) |
| `/setup` | 設定 | API キー、各種設定([data-and-settings.md](data-and-settings.md)) |
| `/shortcuts` | ショートカット | 一覧。定義は `lib/shortcuts.ts` と共有 |
| `/debug` | デバッグ | デバッグモードが有効なときだけ([debug-log.md](debug-log.md)) |

左のナビゲーション(`AppShell`)で移動する。幅の狭いウィンドウではアイコンだけになる。

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
| `Mod`+`/` | ショートカット一覧を開く | どの画面でも |

- 判定は `lib/shortcuts.ts` の `matchShortcut`。IME の変換中(`isComposing`)や、入力欄・セレクトボックスへのフォーカス中は `Space` を奪わない。
- ボタンのクリックは従来どおり開始 / 停止の切り替え。

## エラーと通知

- バックエンドのエラーは `{ code, message, source? }`。`reportError` が表示用の文言にし、タイムアウト・レート制限・サーバーエラー・ネットワークエラー・認証失敗(`AUDIBLE_ERRORS`)では通知音を鳴らす。
- 通知は `sonner` のトースト。

## 設定画面の構成

各節は `Heading`(タイトル + ヘルプ)で区切る。

| 節 | 内容 |
| --- | --- |
| AI Link / Voice Link | API キー(Anthropic / OpenAI / Google Cloud / Aqua Voice)、音声認識エンジン・言語、Whisper モデルのダウンロード、定型音声の作成 |
| Web Search / Command Tasks / Command Policy | ツールの有効化、コマンドの許可・ask・拒否 |
| Knowledge Interval | 整理の間隔 |
| Debug Mode | デバッグログ |
| Personal Data | パーソナルデータ([user-profile.md](user-profile.md)) |
| Base Prompt | ベース指示 |
| Proofread | 校正モデル |
| Wake Words | 常時待機と起動ワード |
| Voice Timing / Limits | 調整項目 |
| Backup | 設定のエクスポート・インポート |

キーの登録時は、疎通確認に成功したときだけ保存する。

## メニューバー常駐(`src-tauri/src/tray.rs`)

- メニューバー(トレイ)にアイコンを常駐させる。アイコンは `icons/tray.png`(黒+透過のテンプレート画像。macOS がメニューバーの配色に合わせて描画する)。
- ウィンドウの「閉じる」操作は**終了ではなく隠す**ことに差し替える。macOS では Dock アイコンも消してメニューバーだけにする(`ActivationPolicy::Accessory`)。
- 左クリックでウィンドウを前面に出す(Dock アイコンも復帰)。右クリックでメニュー: 「MyAssistant を開く」「終了」。
- 終了はトレイメニューの「終了」から。終了前に未処理ログを整理する([architecture.md](architecture.md#起動と終了))。
