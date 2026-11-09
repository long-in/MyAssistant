# 通信のプロキシ

アプリの外向きの通信をプロキシ経由にする、全体設定の機能。設定画面の「Global Settings → アプリ → Proxy」で操作する。実装は `services/proxy.rs` と `commands/proxy.rs`。

## 設定項目

| 項目 | 保存先 | 内容 |
| --- | --- | --- |
| `proxy_enabled` | `workspaces/global_settings.json` | 有効(`1`)/無効(`0`) |
| `proxy_url` | 同上 | `http://ホスト:ポート` または `https://…`。認証情報は含められない |
| `proxy_username` | 同上 | 認証用のユーザー名(空なら認証なし) |
| パスワード | `<アプリデータ>/proxy/secrets.json` | 権限 0600。`services/secrets.rs` の外へ出さず、画面へは登録の有無(bool)だけ返す |

- **無効にしても設定は消えない。** 通信にプロキシを使わなくなるだけで、URL・ユーザー名・パスワードは保持する。再び有効にすると、保持した設定でそのまま使える。
- 有効にするにはURLが必要。無効のままURLだけを保存することもできる。URLが不正な保存は拒否する。
- パスワードは「据え置き(未入力)/更新/削除」を区別する(`set_proxy_settings` の `password`: `None` / 文字列 / 空文字)。
- 端末ごとの設定で、認証情報を含みうるため、設定のバックアップ(エクスポート)には含めない。

## 反映のしかた

- 起動時に `proxy::init` が保存済みの設定を読み、保存のたびに `proxy::reload` がメモリの設定を入れ替える。通信は呼び出しのたびにクライアントを作るので、**保存した次の通信から**反映される(再起動は不要)。
- reqwest を使う通信(Anthropic・Google TTS/STT・Aqua・キー確認・Whisper/VOICEVOX/ローカルLLMの取得)は `proxy::client_builder()` / `proxy::get()` を通す。
- AWS SDK(Bedrock・STS)は、`proxy::aws_http_client()` が返すHTTPクライアントを設定する(無効なら SDK の既定のまま)。
- `localhost` / `127.0.0.1` / `::1` 宛て(VOICEVOX・ローカルLLM のサーバーへの通信)は常にプロキシを通さない。
- 無効のときは従来どおり(reqwest は環境変数 `HTTP(S)_PROXY` 等に従う。AWS SDK はプロキシ無し)。

## 制約

- 対応するスキームは `http` / `https` のプロキシのみ(SOCKS は非対応)。認証は Basic。
- Web 検索・ページ取得は Anthropic のサーバー側ツールで行うため、アプリからの通信は Anthropic への接続のみ(プロキシ対象)。
