# コントリビュートについて

Issue と Pull Request を歓迎します。

## 前提

- 対応 OS は macOS のみです([README.md](README.md) の「必要環境」を参照)。
- 構成上の制約とセキュリティ規約は [AGENTS.md](AGENTS.md) にまとまっています。特に、API キーの生値を `secrets.rs` の外へ出さないこと、ナレッジのパスを必ずサンドボックスに通すことを守ってください。

## 開発

```bash
npm install
npm run tauri dev
```

## PR の前に

```bash
npm run lint
cd src-tauri && cargo test
```

- 実 API を使うテストは `#[ignore]` にします。通常のテストでネットワークやキーを要求しないでください。
- `secrets.json`、`timeline.db`、`.env*` をコミットしないでください。
- Rust のコメントは日本語の doc コメントで、既存コードの密度に合わせます。
