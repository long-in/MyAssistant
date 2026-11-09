#!/usr/bin/env python3
"""依存ライブラリのライセンス一覧(THIRD_PARTY_LICENSES.md)を生成する。

Rust は `cargo metadata`、npm は `package-lock.json` と node_modules から取得する。
cargo が PATH に無い場合は ~/.rustup/toolchains/stable-*/bin を PATH に追加すること。
"""
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def rust_deps():
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked"],
        cwd=ROOT / "src-tauri", capture_output=True, text=True, check=True,
    ).stdout
    meta = json.loads(out)
    return sorted(
        {(p["name"], p["version"], p["license"] or "不明")
         for p in meta["packages"] if p["name"] != "app"}
    )


def npm_deps():
    lock = json.loads((ROOT / "package-lock.json").read_text())
    rows = set()
    for path, info in lock["packages"].items():
        if not path or info.get("dev"):
            continue
        name = path.split("node_modules/")[-1]
        rows.add((name, info.get("version", ""), info.get("license") or "不明"))
    return sorted(rows)


def table(rows):
    lines = ["| 名前 | バージョン | ライセンス |", "| --- | --- | --- |"]
    lines += [f"| {n} | {v} | {l} |" for n, v, l in rows]
    return "\n".join(lines)


def main():
    text = (
        "# 依存ライブラリのライセンス一覧\n\n"
        "`scripts/generate_licenses.py` が生成します。手で編集しないでください。\n\n"
        "## Rust クレート\n\n" + table(rust_deps()) + "\n\n"
        "## npm パッケージ(本番依存)\n\n" + table(npm_deps()) + "\n"
    )
    (ROOT / "THIRD_PARTY_LICENSES.md").write_text(text)


if __name__ == "__main__":
    main()
