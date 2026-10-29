# セキュリティポリシー

脆弱性を見つけた場合は、公開の Issue ではなく、GitHub の「Report a vulnerability」(Security タブの Private vulnerability reporting)から報告してください。

特に次の点は影響が大きいため、報告を歓迎します。

- API キーの生値がフロントエンドやログへ漏れる経路
- ナレッジのパスサンドボックス(`src-tauri/src/utils/path.rs`)の回避
- コマンド実行の承認ポリシー(`src-tauri/src/services/command_policy.rs`)の回避

設計の詳細は [docs/security.md](docs/security.md) を参照してください。
