# コマンドの許可・拒否(Command Policy)

`run_command` で実行するシェルコマンドの可否を判定する仕組み。実装は `services/command_policy.rs`、画面は設定の Command Policy。使われ方は [agent-command-tasks.md](agent-command-tasks.md)。

## 設計方針

- 許可・ask・拒否・機密・システム領域などの**規則はすべてナレッジの JSON ファイルで管理**し、コマンドごとに読み直す(再起動不要)。ファイルはワークスペースのナレッジごとにあるため、ワークスペース(パーソナル / エージェント)ごとに別のポリシーを持つ。設定のコピーでコマンド権限も写せる([workspace.md](workspace.md))。
- `rm` 系・OS に影響するコマンド・機密ファイルの拒否も、**初期値としてファイルに書き出すだけ**で、ユーザーが編集できる。消せばその保護は無くなる。
- 構文解析による**静的な検査**。解釈できないものは安全側(承認が必要)に倒す。

## 判定(`classify_in`)

コマンドをパイプ・`&&`・`;`・`$(...)`・バッククォート・`sh -c` などで分解し、**最も厳しい判定**を採用する。入れ子は 4 段まで解析し、超えたら個別承認にする。`env` `nohup` `xargs` などのラッパーと環境変数の指定は飛ばして、実際のコマンドを見る。

| 判定 | 動作 |
| --- | --- |
| `Auto` | 承認なしで実行 |
| `Plan` | 承認済み計画の範囲内なら実行。計画が無ければ失敗を返し、計画外なら個別承認 |
| `Ask` | 計画の中でも毎回承認 |
| `Unlisted` | 許可リストにない、読み取り系以外の**単純なコマンド**(通常は `Plan` になるもので、パス引数・リダイレクト先を持たない)が、作業ディレクトリのためナレッジの外に及ぶ。承認されれば許可リストに加えて実行 |
| `Deny` | 実行しない。理由を LLM に返し、ユーザーへ説明させる |

厳しさは `Auto` < `Plan` < `Ask` < `Unlisted` < `Deny`。

```mermaid
flowchart TD
  C["区切り単位のコマンド"] --> D{"deny / deny_words /<br/>protected / システム領域?"}
  D -- はい --> DENY["Deny"]
  D -- いいえ --> K{"ask に一致?<br/>パイプ先のインタプリタ?<br/>上書きリダイレクト?"}
  K -- はい --> ASK["Ask"]
  K -- いいえ --> A{"allow に先頭一致?"}
  A -- はい --> AUTO["Auto"]
  A -- いいえ --> PLAN["Plan"]
  ASK --> S
  PLAN --> S
  AUTO --> OK["確定"]
  S{"ナレッジの外に及ぶ?<br/>(restrict_to_knowledge)"}
  S -- いいえ --> OK
  S -- "はい、かつ Plan で<br/>許可パターンを作れる" --> U["Unlisted"]
  S -- はい --> DENY2["Deny"]
```

## 既定の規則

### 許可(Auto): 読み取り系

`ls` `pwd` `cat` `head` `tail` `wc` `file` `stat` `du` `df` `which` `whoami` `date` `uname` `sw_vers` `echo` `grep` `rg` `find` `tree` `git status` `git log` `git diff` `git show`。版 3 で `ps` `id` `hostname` `uptime` `printenv` `basename` `dirname` `realpath` `readlink` `diff` `cut` `tr` `lsof` と、`git rev-parse` `git ls-files` `git blame` `git remote -v` `git branch --list` `git tag -l` など読み取りだけのサブコマンドを加えた。

### 拒否(Deny)

すべて編集でき、消せばその保護は無くなる。

| フィールド | 既定 | 備考 |
| --- | --- | --- |
| `deny_words` | `rm` `rmdir` `unlink` `shred` `sudo` `su` `doas` | **コマンドのどの位置にあっても**拒否(`xargs rm`、`git rm`、`sh -c 'rm …'` 経由も) |
| `deny` | `shutdown` `reboot` `halt` `poweroff` `launchctl` `systemsetup` `diskutil` `csrutil` `nvram` `pmset` `scutil` `networksetup` `dscl` `spctl` `tmutil` `mount` `defaults write` など `find -delete` | 引数の途中でも一致 |
| `protected` | `secrets.json` `timeline.db` `sandbox.env` `.aws/credentials` `.ssh/id_` `.netrc` `.gnupg/` | 引数に含むコマンドを拒否(API キー・DB・サンドボックスの環境変数・端末の認証情報の漏えい防止) |
| `system_paths` | `/System` `/usr` `/bin` `/sbin` `/etc` `/Library` `/private/etc` `/dev` | ここへの書き込み・リダイレクトを拒否 |
| `system_exceptions` | `/usr/local` | `system_paths` のうち書き込みを許す |

### 個別承認(Ask)

`ask` の既定: `git push` `git reset --hard` `git clean` `git checkout --` `git restore` `git branch -D` `git stash drop` `git stash clear` `git rebase` `mv` `dd` `truncate` `kill` `killall` `pkill` `osascript` `chmod -R` `chown -R` `brew uninstall` `brew remove` `npm publish` `cargo publish`。

フラグ: `confirm_overwrite`(リダイレクト `>` での上書きを毎回承認)、`confirm_pipe_to_shell`(パイプで受けたコードの実行、`curl … | sh` など)。

### アプリデータの保護(編集できない)

`protected` は文字列の一致なので、ワイルドカード(`secrets.jso?`)や作業ディレクトリの指定で避けられる。そのため、ポリシーファイルとは別に、**場所**でも守る。この保護は設定で外せない。

- アプリデータのフォルダ(`~/Library/Application Support/MyAssistant/`。そのワークスペース自身のナレッジを除く)を、作業ディレクトリ・パス引数・`--opt=<パス>`・ワイルドカードの手前のフォルダ・シンボリックリンクのどれで指しても、読み取り系を含めて `Deny`。ほかのワークスペースのナレッジ・DB・キーにも届かない。
- アプリデータを含む範囲(ホームなど)を再帰的に読むコマンド(`rg`、`grep -r`、`find … -exec`)は `Deny`(対象のフォルダを絞るよう返す)。

### 読み取り系でも承認が要る形

許可リスト(`allow`)に一致しても、次の形は Auto にせず `Plan`(承認済みの計画が必要、範囲の検査も受ける)にする。

| 形 | 例 | 理由 |
| --- | --- | --- |
| 環境変数の前置き | `GIT_EXTERNAL_DIFF='sh -c …' git diff` | 読み取り系のコマンドに別のコマンドを起動させられる |
| 代入だけの区切り | `a=secrets; cat $a.json` | 後のコマンドの引数を実行時に変えられる |
| 変数・コマンド置換の引数 | `cat $HOME/x`、`echo $(ls)` | 行き先を確認できない(正規表現の行末の `$` は対象外) |
| `xargs` | `ls \| xargs cat` | 引数が実行時に決まる |
| 書き出し・別コマンド起動のオプション | `--output`、`-fprint`・`-fprintf`・`-fls`、`--pre`、`--ext-diff`、`--textconv`、`--hostname-bin`、`--open-files-in-pager`、`tree -o` | ファイルへの書き込み・任意のコマンドの実行になる |

## 実行範囲(`Scope`)

フラグ `restrict_to_knowledge`(既定オン)。**読み取り系(Auto)を除き**、作業ディレクトリ(`cwd`)とパス引数・リダイレクト先はナレッジ配下に限る。

- 外を指すもの(`/tmp/x`、`../x`、`~/x`、`$HOME/x` など変数で組み立てたパス)は、`Deny`(理由: 読み取り系以外はナレッジの中でしか実行できない)。ただし、パス引数・リダイレクト先を持たない単純なコマンドが作業ディレクトリだけ外にある場合は、許可リストへ加える承認(`Unlisted`)を求める。`Ask` の規則に当たったものは対象外で、`Deny` になる。
- `..` とシンボリックリンクは解決して判定する(存在する最も深い祖先を正規化して残りを付け直す)。
- `ls /etc` や `cat ~/x` のような**読み取りはナレッジの外でも実行できる**。
- コンテナで動くエージェント(ワークは常に、パーソナルは Bedrock 方式のとき)は、会話をコンテナ内の Claude Code が受け持ち、このファイルは使わない。コマンドの拒否は Claude Code の settings.json(`permissions.deny`)で決める。その雛形と、settings.json が空のときに渡す既定の拒否は、このファイルの初期値の拒否(`deny`)と拒否する語(`deny_words`)を `Bash(<コマンド>:*)` へ変換したもの([sandbox.md](sandbox.md))。
- Bedrock 方式では端末内のモデルも選べないため、コマンドはホストで動かない。画面でも、Bedrock 方式(ワークは常に)では Command Policy を出さない。
- コマンドを区切り単位ごとに見るので、`ls /etc && mkdir a` は後者だけが検査される。

## ポリシーの保存先

エージェントごとのDB(`timeline.db` の `app_settings`)のキー `command_policy`(JSON)。`run_command` のたびに読み直し、未保存なら既定値を書き出す。以前のナレッジの `.my-assistant/command-policy.json` があれば、エージェントを開くときに一度だけ取り込んで削除する(DB に保存があるときは DB を優先し、ファイルには触れない)。

```json
{
  "allow": ["ls", "git status"],
  "ask": ["npm publish"],
  "deny": ["curl"],
  "deny_words": ["rm", "sudo"],
  "protected": ["secrets.json", "timeline.db", "sandbox.env"],
  "system_paths": ["/System", "/usr"],
  "system_exceptions": ["/usr/local"],
  "confirm_overwrite": true,
  "confirm_pipe_to_shell": true,
  "restrict_to_knowledge": true
}
```

設定画面では、ポリシー全体(各リストと 3 つのフラグ)を JSON のまま編集して保存できる(項目ごとの入力欄は無い)。「保存」で、JSON の構文と型(リストは文字列の配列、フラグは true / false)を検証してから保存し、不正なら保存しない。書いていない項目は保存済みの値のままになる。保存は空行・重複を除き(1 リスト 200 件・1 項目 200 文字まで)、次のコマンドから有効になる。ファイルを直接編集してもよい。

### マッチの規則

- **ワイルドカード**: 各項目の語に `*`(任意の文字列)が使える。`aws s3 *` は `aws s3` 以下すべて(末尾の `*` は残りの引数すべて・なしも含む)、`aws * delete-*` は途中の語や語の中の指定。単独の `*` は任意の 1 語。拒否は `*` の許可でも緩まない(拒否リストを編集しない限り)。
- **`allow`**: コマンド名と引数の**先頭**が一致したときだけ Auto(`git -C x status` は一致しない)。
- **`ask` / `deny`**: 引数の途中でも一致する。deny が最優先。
- `allow` に一致したコマンドは読み取り系(Auto)として扱われるため、**ナレッジ内に限る実行範囲の検査も受けない**。広い `allow`(`aws s3 *` など)は、ローカルへの書き込みも承認なしで通る点に注意。

### 保護

- 隠しフォルダなので、LLM のファイルツール(一覧・読み取り・作成)からは触れない。
- ファイルが壊れていれば許可リストは空として扱う(拒否などは初期値のまま)。
- ファイルの内容はそのまま使う。初期値の更新は既存のファイルへ自動では足さない(「初期値に戻す」で反映する)。ファイルにある `version` は読み飛ばす。
- 設定画面の「初期値に戻す」で初期値に戻せる(押すとすぐ保存する)。

### 許可リストへの追加

`Unlisted` のコマンドの承認では、「今回だけ実行」(許可リストへ加えない)と「許可に加えて実行」を選べる。後者を選ぶと、`add_allow` がパターンを許可リストへ加える。この承認は画面のボタンでだけ行える(音声では拒否だけ)。パターンは、サブコマンドで性質が変わるツール(`git` `npm` `pnpm` `yarn` `cargo` `brew` `docker` `gh` `kubectl` `go` `pip`)では `git log` のようにサブコマンドまで含める。シェル・インタプリタ・ラッパーなど、何でも実行できるものは候補にしない。

## バックアップ

設定のエクスポートに含まれる([data-and-settings.md](data-and-settings.md))。取り込み時は同じ検証(`cleaned`)を通して保存する。

## 既知の限界

[agent-command-tasks.md](agent-command-tasks.md#既知の限界) を参照。静的な検査のため、`python -c` の中身や実行時に決まるパスは見ない。
