# AI の接続とモデル・プロンプト

LLM をどの経路で呼ぶか(接続方式)、機能ごとにどのモデルを使うか、各機能へ渡す指示(プロンプト)をどう上書きするかをまとめる。実装は `services/llm_router.rs`(接続方式・カタログ・モデルの検証)、`services/prompts.rs`(プロンプト)、`services/sandbox_agent.rs`(コンテナ内の Claude Code のモデル)、画面は設定の「LLM」タブ(AI Link・AI Settings。`src/app/setup/page.tsx`、`src/components/AiSettings.tsx`)。

## 接続方式(AI Link)

エージェントごとに、LLM を呼ぶ経路を **API** か **Amazon Bedrock** のどちらか一方に決める(`llm_router::LinkMode`、設定キー `ai_link_mode`)。

| 方式 | 使う LLM | 処理の場所 |
| --- | --- | --- |
| API | Anthropic(API キー)と端末内のモデル(Qwen。[local-llm.md](local-llm.md)) | ホスト。このアプリのツールループ([agent-file-tools.md](agent-file-tools.md)) |
| Amazon Bedrock | コンテナ内の Claude Code(Bedrock。ホストの中継が署名する。[sandbox-aws.md](sandbox-aws.md)) | エージェント専用の Docker コンテナ([sandbox.md](sandbox.md)) |

- **ワークのエージェントは常に Bedrock**(変えられない。`set_link_mode` は拒否する)。
- パーソナルとリーダーは選べる。未保存のときは、AWS の設定だけを登録していれば Bedrock、それ以外(両方登録済みを含む)は API。
- Bedrock 方式では、LLM の機能(チャット・整理・宛先の提案。校正はオンにしたとき)をすべてコンテナ内の Claude Code で行う(`llm_router::uses_container`)。スキルの選定はホストの経路だけの機能なので行わない([skills.md](skills.md))。校正はオフかコンテナ内の Claude Code。API のモデルも端末内のモデルも選べない。
- Amazon Bedrock は、このアプリから直接は呼ばない(Bedrock のモデル一覧も持たない)。AWS の設定と認証は [security.md](security.md#aws-認証awsrs)。
- 画面(AI Link): 方式を選ぶと、その方式のカードだけを出す(API キー・端末内の LLM の取得、または AWS の設定)。登録済みのキー・設定は、方式を切り替えても消えない。各カードの「使用中の機能」は、AI Settings でそのプロバイダーのモデルを選んでいる機能。
- キーは疎通確認に成功したときだけ保存する([security.md](security.md#api-キーsecretsrs))。

## モデルのカタログ(`llm_router`)

API 方式で選べるモデルは、カタログにあるものだけ。モデルの入れ替えは `llm_router.rs` の `FIXED` だけを編集する。

| プロバイダー | モデル | 区分 |
| --- | --- | --- |
| Anthropic | `claude-haiku-5-5` | 軽量 |
| Anthropic | `claude-sonnet-5-5`、`claude-opus-5-5` | 標準 |
| ローカル | `qwen2.5-7b-instruct`(取得済みのときだけ。カタログの末尾) | 標準 |

- 保存したモデル(`Route` = プロバイダー + モデル ID)は、使うたびにカタログとキーの登録で検証する(`llm_router::resolve`)。未登録のプロバイダーなら `key_missing`。
- 保存済みの選択が使えない(キーを削除した、ローカルのモデルを削除した等)ときは既定へ戻す。既定は、Claude が使えれば Sonnet、なければ使えるプロバイダーのカタログ先頭。
- Bedrock 方式のパーソナル・リーダーでは、選択肢は「コンテナ内の Claude Code」だけ(`Route.provider` が `sandbox`)。Claude Code の settings.json で許可したモデル(`model` と `availableModels`)ごとに並び、許可の指定が無ければ settings.json の `model` に任せる1件になる。以前の Anthropic・Bedrock・端末内のモデルの保存は、コンテナの経路(settings.json の既定)に読み替える。

## 機能ごとのモデル

| 機能 | 設定キー | 選ぶ場所 | API 方式の既定 | Bedrock 方式 |
| --- | --- | --- | --- | --- |
| チャット(と会話の要約) | `routing` | チャット画面の入力欄のモデル選択(`ModelPicker`) | Sonnet | パーソナル・リーダーは許可したモデルから選ぶ。ワークは AI Settings の「チャット」で選ぶ(`sandbox_model`。未選択なら settings.json の `model`) |
| 校正 | `proofread_route` | AI Settings の「文字起こしの校正」 | オフ(選ぶとオン。初期値は軽量モデル) | オフ / コンテナ内の Claude Code |
| ナレッジの整理(抽出と統合) | `batch_route` | AI Settings の「ナレッジの整理」 | Sonnet | コンテナ内の Claude Code |
| スキルの選定 | `skills_route` | AI Settings の「スキルの選定」(パーソナル・エージェント) | Haiku、無ければ使えるモデルの先頭 | 選べるのはコンテナ内の Claude Code だけだが、会話がコンテナで動くため選定自体を行わない |
| 宛先の提案(リーダー) | なし(自動) | — | 軽量モデル、無ければ既定 | コンテナ内の Claude Code(上限90秒) |

- モデルはエージェントごとに選ぶ(エージェントごとに使えるキーが違うため)。
- 会話の要約は、チャットと同じモデルを使う。統合は、整理と同じモデルを使う。
- コンテナ内の Claude Code を機能ごとに指定したとき、`Route.model` が settings.json で許可したモデルなら `--model` で渡し、空・`default`・許可から外れたものは settings.json の `model` に任せる(`sandbox_agent::route_model`)。

## プロンプト

LLM を使う機能ごとに、既定の指示を持つ。エージェントごと・全体設定で、全文を書き換えられる(`services/prompts.rs`)。

| 機能(`PromptId`) | エージェントのキー | 全体設定のキー | 使う場所 |
| --- | --- | --- | --- |
| チャット(`chat`) | `prompt_chat` | `base_instructions`(ベース指示) | [chat-flow.md](chat-flow.md) |
| 会話の要約(`summary`) | `prompt_summary` | `global_prompt_summary` | [chat-flow.md](chat-flow.md#長い会話の要約compactrs) |
| 校正(`proofread`) | `prompt_proofread` | `global_prompt_proofread` | [voice-input.md](voice-input.md#校正proofreadrs) |
| 整理の抽出(`organize`) | `prompt_organize_v2` | `global_prompt_organize_v2` | [knowledge.md](knowledge.md#整理バッチbatchrs) |
| 整理の統合(`consolidate`) | `prompt_consolidate` | `global_prompt_consolidate` | [knowledge.md](knowledge.md#整理バッチbatchrs) |
| スキルの選定(`skills`) | `prompt_skills` | `global_prompt_skills` | [skills.md](skills.md) |
| 宛先の提案(`suggest`) | `prompt_suggest` | `global_prompt_suggest` | [leader.md](leader.md#宛先の提案leader_suggest) |

- 解決の順は、エージェントの上書き → 全体設定 → 組み込みの既定。エージェントが上書きしていなければ「全体設定を利用」で、全体設定を変えると次の呼び出しから自動で反映される。上書きしたエージェントは「個別設定」になり、全体設定の変更では更新されない。「全体設定を利用」(`reset_prompt`)で上書きを外す。
- 空、または既定と同じ文章は「上書きなし」として扱う(既定の更新にも追従する)。1つのプロンプトは 20,000 文字まで。
- 会話ログなどのデータ部分は各機能が後ろへ付けるので、プロンプトの編集では変えられない。出力の形式(JSON など)を示す部分を消すと、結果を読み取れず機能が働かなくなることがある。
- 画面: エージェントの設定の「LLM」タブの AI Settings(機能ごとのモデルとプロンプト。スキルの選定はパーソナル・エージェントだけ、宛先の提案はリーダーだけ)。全体設定の「AIへの指示」タブの Base Prompt(チャット)と Shared Prompts(チャット以外)。
- 上書きはエージェントの設定の書き出し・コピーの対象。全体設定の側はアプリ全体のバックアップに含まれる([data-and-settings.md](data-and-settings.md))。

## 関連する設定

- Claude Code の settings.json(モデル・許可するツール)と、1回の指示の上限時間は、Sandbox タブで設定する([sandbox.md](sandbox.md))。
- 検索回数・会話の往復数の上限(AI Limits)は [data-and-settings.md](data-and-settings.md#調整項目tuningrs)。
