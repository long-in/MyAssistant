import { Channel, invoke } from "@tauri-apps/api/core";
import { tr } from "@/lib/i18n";

/** `local`は端末内のLLM。キーは無く、`KeyStatus`は取得済みかどうかを表す。 */
export const PROVIDERS = ["anthropic", "bedrock", "local"] as const;
export type Provider = (typeof PROVIDERS)[number];
export type KeyStatus = Record<Provider, boolean>;

export type AppError = { code: string; message: string; source?: string | null };

/** エラーの発生元APIの表示名。 */
export const SOURCE_LABELS: Record<string, string> = {
  anthropic: "Anthropic (Claude)",
  bedrock: "Amazon Bedrock (Claude)",
  local: "Local (Qwen)",
  google_tts: "Google Cloud Text-to-Speech",
  google_stt: "Google Cloud Speech-to-Text",
  voicevox: "VOICEVOX",
};

export const PROVIDER_LABELS: Record<Provider, string> = {
  anthropic: "Anthropic (Claude)",
  bedrock: "Amazon Bedrock (Claude)",
  local: "Local (Qwen)",
};

/** モデルの選択肢の見出し。`sandbox`は、コンテナ内の Claude Code(Bedrockもここで使う)。 */
export const MODEL_PROVIDER_LABELS: Record<ModelProvider, string> = {
  ...PROVIDER_LABELS,
  sandbox: "コンテナ内の Claude Code",
};

export const ERROR_MESSAGES: Record<string, string> = {
  empty_key: "APIキーを入力してください。",
  unauthorized: "APIキーが無効です。内容を確認してください。",
  rate_limited: "レート制限またはクォータ超過です。しばらくしてから再試行してください。",
  timeout: "接続がタイムアウトしました。",
  network_error: "ネットワークに接続できません。",
  server_error: "プロバイダー側でエラーが発生しました。",
  secret_store_error: "APIキーの保存ファイルの読み書きに失敗しました。",
  db_error: "データベースの操作に失敗しました。",
  no_provider: "APIキーが登録されていません。初期設定から登録してください。",
  key_missing: "選択したプロバイダーのAPIキーが未登録です。",
  invalid_model: "選択したモデルは利用できません。",
  empty_message: "メッセージを入力してください。",
  bad_response: "プロバイダーから想定外の応答が返りました。",
  audio_error: "マイクを利用できません。マイクの権限と接続を確認してください。",
  already_recording: "すでに録音中です。",
  not_recording: "録音していません。",
  no_audio: "音声を認識できませんでした。もう一度お試しください。",
  too_short: "録音が短すぎます。もう少し長く話してから停止してください。",
  mic_no_signal:
    "マイクから音が入力されていません(無音)。macOSの「システム設定 → プライバシーとセキュリティ → マイク」で、アプリを起動しているターミナル(開発中)またはこのアプリに許可を与え、入力デバイスとミュートも確認してください。",
  mic_too_quiet: "マイクの音量が小さすぎます。入力音量とマイクの位置を確認してください。",
  stt_model_missing: "音声認識モデルが未インストールです。",
  stt_error: "音声認識に失敗しました。",
  too_long: "録音が長すぎます(Google音声認識は約1分まで)。短く区切って話してください。",
  empty_text: "読み上げるテキストがありません。",
  voicevox_error: "VOICEVOXでの音声合成に失敗しました。設定でVOICEVOX COREを取得し直してください。",
  voicevox_missing: "VOICEVOX COREが未取得です。設定の「Voice Link」で取得してください。",
  playback_error: "音声を再生できませんでした。メッセージ横のスピーカーボタンから再生してください。",
  api_disabled: "このキーのプロジェクトで対象のAPIが有効化されていません。Google Cloud Consoleで「Cloud Text-to-Speech API」または「Cloud Speech-to-Text API」を有効にしてください。",
  key_restricted: "APIキーの制限(API制限・アプリケーション制限)によりブロックされました。キーの制限設定を確認してください。",
  billing_disabled: "プロジェクトの課金(請求先アカウント)が有効になっていません。",
  local_llm_missing: "ローカルLLMが未取得です。設定の「AI Link」で取得してください。",
  local_llm_error: "ローカルLLMの起動または応答に失敗しました。",
  checksum_mismatch: "取得したファイルの検証に失敗しました。もう一度取得してください。",
  download_cancelled: "取得を中断しました。",
  invalid_version: "バージョンには英数字と . _ - のみ使えます(64文字まで)。",
  path_not_allowed: "ナレッジディレクトリ外のパスにはアクセスできません。",
  io_error: "ファイルの読み書きに失敗しました。",
  mfa_required: "MFAコードが入力されませんでした。もう一度お試しください。",
  invalid_dictionary: "読み上げ辞書のJSONを取り込めませんでした。[{\"word\": \"語\", \"reading\": \"読み\"}] の形式で、ファイルは1MB以下にしてください。",
  invalid_settings: "設定ファイルの形式が正しくありません。このアプリが書き出したファイルを選んでください。",
  backup_password_required: "このバックアップは暗号化されています。パスワードを入力してください。",
  backup_password_wrong: "パスワードが違います。",
  invalid_backup: "バックアップファイルを使えません。このアプリが書き出した、壊れていない新しすぎないzipを選んでください。",
  // エージェント(ワークスペースの管理)の画面に出る文言。表記は言語設定に従う
  invalid_agent_data: "指示は2,000文字以内で、< > と制御文字を含めずに入力してください。",
  invalid_skill: "スキルの内容を確認してください。名前は小文字の英数字とハイフンのみ64文字以内、説明は1,024文字以内(「<」「>」不可)、本文は3,000文字以内で、同じ名前は使えません。",
  invalid_agent: "エージェントの名称(1〜40文字)またはイメージカラーが正しくありません。",
  reserved_name: "その名称は予約されています。別のエージェント名にしてください。",
  name_in_use: "その名称は、ほかのエージェントがすでに使っています。別の名称にしてください。",
  color_in_use: "その色は、ほかのエージェントがすでに使っています。別の色を選んでください。",
  agent_limit: "エージェントは30件までです。不要なエージェントを削除してください。",
  agent_not_found: "エージェントが見つかりません。",
  leader_required: "エージェントを作るには、先にリーダーを作成してください。",
  leader_exists: "リーダーはすでに作成されています。",
  work_disabled: "ワークは無効です。全体設定で有効にしてください。",
  work_busy: "ワークの中に実行中または承認待ちの依頼があります。完了してから無効にしてください。",
  sandbox_error: "サンドボックスで問題が発生しました。",
  path_in_use: "その場所は、ほかのワークスペースのナレッジまたはデバッグログの保存先と重なっています。別の場所を選んでください。",
};

export function errorMessage(e: unknown): string {
  const { code, message, source } = (e ?? {}) as Partial<AppError>;
  const body =
    code === "provider_rejected" && message
      ? tr("拒否されました: {message}", { message })
      : code === "sandbox_error" && message
        ? tr(message)
      : code === "aws_config_error" && message
        ? tr("AWSの設定に問題があります: {message}", { message })
      : tr((code && ERROR_MESSAGES[code]) || "予期しないエラーが発生しました。");
  const label = source ? SOURCE_LABELS[source] : undefined;
  return label ? `[${label}] ${body}` : body;
}

/** 登録有無のみ。キーの生値はフロントエンドへ渡されない。 */
export const getKeyStatus = () => invoke<KeyStatus>("get_key_status");

/** Rust側で疎通確認し、成功時のみOSストレージへ保存される。 */
export const saveApiKey = (provider: Provider, key: string) =>
  invoke<void>("save_api_key", { provider, key });

export const deleteApiKey = (provider: Provider) =>
  invoke<void>("delete_api_key", { provider });

/** 端末内のLLM(llama-serverとモデル)の取得状況。`size`は取得する合計(バイト)。 */
/** 取得の進み具合。`bytes`が偽のときは単位のない割合(複数ファイルの工程など)で、`downloaded / total`だけを使う。 */
export type DownloadProgress = { downloaded: number; total: number; bytes: boolean };
export type LocalLlmStatus = { installed: boolean; downloading: boolean; label: string; size: number; progress: DownloadProgress | null };
export const localLlmStatus = () => invoke<LocalLlmStatus>("local_llm_status");
/** 取得の進み具合を`onProgress`へ流す。取得済みのモデルは取り直さない。 */
export const downloadLocalLlm = (onProgress: (p: DownloadProgress) => void) => {
  const channel = new Channel<DownloadProgress>();
  channel.onmessage = onProgress;
  return invoke<void>("download_local_llm", { onProgress: channel });
};
export const cancelLocalLlmDownload = () => invoke<void>("cancel_local_llm_download");
export const removeLocalLlm = () => invoke<void>("remove_local_llm");

/** `~/.aws/config`・`credentials`の貼り付け内容から読み取ったプロファイル。キーの値は含まれない。 */
export type AwsProfile = {
  name: string;
  region: string | null;
  role: boolean;
  mfa: boolean;
  usable: boolean;
  /** 使えない場合の理由(英語の短い説明)。 */
  reason: string | null;
};

export type AwsStatus = {
  registered: boolean;
  profiles: AwsProfile[];
  selected: string | null;
  /** 一時クレデンシャルの期限(UNIX秒)。未取得ならnull。 */
  session_expires_at: number | null;
};

/** MFAコードの入力を求めるイベント(AWSの認証はコードが届くまで待つ)。 */
export const AWS_MFA_EVENT = "aws-mfa-required";
export type AwsMfaPrompt = {
  profile: string;
  mfa_serial: string;
  error: string | null;
  /** コードを求めているワークスペース(`personal`またはエージェントID)。コードはこのIDで返す。 */
  workspace_id: string;
  /** エージェントの名称とイメージカラー。パーソナルは`null`。 */
  workspace_name: string | null;
  workspace_color: string | null;
};

export const getAwsStatus = () => invoke<AwsStatus>("get_aws_status");
/** 保存済みの`~/.aws/config`・`credentials`の全文。本人が確認・編集するためだけに使う(ログや表示以外へ渡さない)。 */
export const getAwsFiles = () => invoke<{ config: string; credentials: string }>("get_aws_files");
/** このワークスペースの`secrets.json`をFinderで開く。 */
export const revealSecretsFile = () => invoke<void>("reveal_secrets_file");
/** 貼り付け中の内容から読み取れるプロファイル一覧(保存しない)。 */
export const previewAwsConfig = (config: string, credentials: string) =>
  invoke<AwsProfile[]>("preview_aws_config", { config, credentials });
/** 貼り付けた内容と使うプロファイルをRust側で検証して保存する。保存後、内容はフロントエンドへ戻らない。 */
export const saveAwsConfig = (config: string, credentials: string, profile: string) =>
  invoke<AwsStatus>("save_aws_config", { config, credentials, profile });
export const selectAwsProfile = (name: string) => invoke<AwsStatus>("select_aws_profile", { name });
/** AWSの認証(プロファイル・ロールの引き受け)が通るかを確認する。MFAが必要なら`AWS_MFA_EVENT`が飛び、コード入力まで待つ。 */
export const testAwsConnection = () => invoke<AwsStatus>("test_aws_connection");
/** コードは、求めたワークスペースへ返す(選択中とは限らない)。 */
export const submitAwsMfa = (workspaceId: string, code: string) => invoke<void>("submit_aws_mfa", { workspaceId, code });
export const cancelAwsMfa = (workspaceId: string) => invoke<void>("cancel_aws_mfa", { workspaceId });

/**
 * モデルの選択肢の接続先。`Provider`に加えて、`sandbox`(コンテナ内の Claude Code。モデルは settings.json で決まる)がある。
 * Bedrockはこのアプリから直接は呼ばず、`sandbox`で使う。`sandbox`を選べるのは、パーソナルとリーダーだけ。
 */
export type ModelProvider = Provider | "sandbox";
export type ModelKeyStatus = Partial<Record<ModelProvider, boolean>>;
export const SANDBOX_PROVIDER = "sandbox" as const;

export type Route = { provider: ModelProvider; model: string };

export type ModelInfo = {
  provider: ModelProvider;
  id: string;
  label: string;
  tier: "light" | "heavy";
};

export type ChatReply = { content: string; provider: string; model: string };

/** 聞き取り間違いの訂正をナレッジ(Markdown)へ記録する。保存した場合はtrue。 */
export const addSttCorrection = (heard: string, correct: string) =>
  invoke<boolean>("add_stt_correction", { heard, correct });

/** AI Link の接続方式。API キー(Anthropic)か Amazon Bedrock(コンテナ内の Claude Code)のどちらか一方だけを使う。ワークは常に`bedrock`。 */
export type LinkMode = "api" | "bedrock";
export const loadLinkMode = () => invoke<LinkMode>("load_link_mode");
export const saveLinkMode = (mode: LinkMode) => invoke<void>("save_link_mode", { mode });
export const listModels = () => invoke<ModelInfo[]>("list_models");
export const loadRouting = () => invoke<Route | null>("load_routing");
export const saveRouting = (route: Route) => invoke<void>("save_routing", { route });
/**
 * Claudeを選んでいるときはナレッジのファイル操作も行い、その進捗を`onEvent`へ流す。
 * `skill`は入力欄で指定したスキルの名前。指定すると選定を省いて、その手順で進める。`null`なら発話に合うものを自動で選ぶ。
 */
export const sendChat = (sessionId: string, text: string, route: Route, skill: string | null, onEvent: (e: TaskEvent) => void) => {
  const channel = new Channel<TaskEvent>();
  channel.onmessage = onEvent;
  return invoke<ChatReply>("send_chat", { sessionId, text, route, skill, onEvent: channel });
};

/** 承認の種類。file=ファイル作成、plan=作業計画、command=計画外・破壊的なコマンドの個別承認、allow_command=許可リストにないコマンドを許可に加えて実行、skill_update=スキルの上書き、skill_delete=スキルの削除。 */
export type ApprovalKind = "file" | "plan" | "command" | "allow_command" | "skill_update" | "skill_delete";

/** 配布の下書きの宛先(エージェント)。 */
export type DraftTarget = { id: string; name: string; color: string };
/** リーダーの配布の下書き。`targets`は、AIが推奨する宛先。 */
export type DispatchDraft = { id: string; instruction: string; targets: DraftTarget[] };

/** 音声対話中のファイル操作・作業の進捗。`approval`は実行前の承認要求で、`taskApprove`で応答する。 */
export type TaskEvent =
  | { type: "text"; text: string }
  | { type: "tool_call"; id: string; name: string; input: Record<string, unknown> }
  | { type: "tool_result"; id: string; ok: boolean; preview: string }
  | {
      type: "approval";
      id: string;
      kind: ApprovalKind;
      /** ファイルは作成先、コマンドは作業ディレクトリ。計画では空。 */
      path: string;
      /** ファイルは全文、コマンドはコマンド、計画は概要。 */
      preview: string;
      steps: string[];
      /** 個別承認が必要な理由(コマンドのみ)。 */
      reason: string;
    }
  | { type: "command_output"; id: string; stream: "stdout" | "stderr"; text: string }
  /** リーダーが作った配布の下書き。利用者が宛先を確かめて配布する(AIは配布できない)。 */
  | ({ type: "dispatch_draft" } & DispatchDraft)
  /** 依頼の完了。切り替えから戻って`taskResume`で接続し直した画面にだけ届く(最初の画面は`sendChat`の戻り値で受け取る)。 */
  | { type: "finished"; reply: ChatReply | null; error: AppError | null };

/** 切り替えから戻ったときに受け取る、そのワークスペースの依頼の状況。 */
export type RunSnapshot = {
  /** 実行中か(切り替えている間に終わっていれば偽)。 */
  running: boolean;
  session_id: string;
  user_text: string;
  /** これまでの進捗(新しい側の最大500件)。 */
  events: TaskEvent[];
  /** 回答待ちの承認。 */
  pending: Omit<Extract<TaskEvent, { type: "approval" }>, "type"> | null;
  /** 画面が見ていない間に終わった依頼の結果(一度だけ渡される)。 */
  finished: { reply: ChatReply | null; error: AppError | null } | null;
};

/**
 * ワークスペースへ戻ったとき、実行中だった依頼の状況を受け取り、以降の進捗(と結果)を受ける接続を張り直す。
 * 切り替えても依頼は止まらず、承認が要るときは回答を待っている。
 */
export const taskResume = (workspaceId: string, onEvent: (e: TaskEvent) => void) => {
  const channel = new Channel<TaskEvent>();
  channel.onmessage = onEvent;
  return invoke<RunSnapshot>("task_resume", { workspaceId, onEvent: channel });
};
/** 画面がワークスペースを離れたことを知らせる。依頼は止まらず、進捗はバックエンドに記録される。 */
export const taskDetach = (workspaceId: string) => invoke<void>("task_detach", { workspaceId });

export const taskApprove = (id: string, approved: boolean) =>
  invoke<void>("task_approve", { id, approved });
/** 実行中の依頼を中断する。動いているコマンドは強制終了され、承認待ちは拒否になる。 */
export const taskCancel = () => invoke<void>("task_cancel");

/** シェルコマンドによる作業の可否(Claude選択時のみ有効)。未設定なら有効。 */
export const getShellAccess = () => invoke<boolean>("get_shell_access");
export const setShellAccess = (enabled: boolean) => invoke<void>("set_shell_access", { enabled });

/** サンドボックス(エージェント専用のDockerコンテナでコマンドを実行する)の設定。エージェントだけが使える。既定は無効。 */
export const getSandboxEnabled = () => invoke<boolean>("get_sandbox_enabled");

export type DockerState = "missing" | "stopped" | "ready";
export type SandboxStatus = {
  docker: DockerState;
  container_running: boolean;
  /** 起動中で、いまのイメージから作られたコンテナがあるか(チャットを使える状態)。 */
  container_ready: boolean;
  /** このエージェントのコンテナの名前(`myassistant-<ランダム>`)。まだ作っていなければ null。 */
  container_name: string | null;
  /** 専用のDockerfileが登録されているか(あれば共通より優先)。 */
  dockerfile_registered: boolean;
  /** このエージェントが使うイメージがビルド済みか。 */
  image_ready: boolean;
  building: boolean;
};
export const getSandboxStatus = () => invoke<SandboxStatus>("get_sandbox_status");

/** 全エージェント共通のイメージの状態。 */
export type SandboxImageStatus = {
  docker: DockerState;
  /** 現在のDockerfileのイメージがビルド済みか。 */
  image_ready: boolean;
  building: boolean;
};
export const getSandboxImageStatus = () => invoke<SandboxImageStatus>("get_sandbox_image_status");

export type DockerfileState = "missing" | "template" | "edited";
/** 全エージェント共通のDockerfile。 */
export const getSandboxDockerfile = () => invoke<{ content: string; state: DockerfileState }>("get_sandbox_dockerfile");
export const setSandboxDockerfile = (content: string) => invoke<DockerfileState>("set_sandbox_dockerfile", { content });

/** sandbox.env の名前と検証結果(値が空の名前は含まない)。 */
export type SandboxEnvSummary = { names: string[]; error: { line: number | null; message: string } | null };
/** sandbox.env の内容(値を含む)。無ければ既定の内容で作る。 */
export const getSandboxEnv = () => invoke<{ content: string; summary: SandboxEnvSummary }>("get_sandbox_env");
/** sandbox.env を保存する。書式に誤りがあれば保存せずエラー。 */
export const setSandboxEnv = (content: string) => invoke<SandboxEnvSummary>("set_sandbox_env", { content });

export type SandboxFiles = {
  /** 環境変数ファイルの名前と検証結果。値は含まれない。 */
  env: { names: string[]; error: { line: number | null; message: string } | null };
};
export const getSandboxFiles = () => invoke<SandboxFiles>("get_sandbox_files");

/** イメージをビルドする。進捗は`SANDBOX_BUILD_EVENT`で流れる。`force`ならビルド済みでも作り直す。 */
export const buildSandboxImage = (force: boolean) => invoke<void>("build_sandbox_image", { force });
export const SANDBOX_BUILD_EVENT = "sandbox-build-progress";
/** `scope`は`shared`(共通のイメージ)またはエージェントID(専用のイメージ)。 */
export type SandboxBuildProgress = { scope: string; line: string };
/** このエージェントのコンテナを作り直す(専用ボリュームは残る)。 */
/** イメージのビルドが済んでいるとき、このエージェントのコンテナを作って起動する。 */
export const startAgentSandbox = () => invoke<void>("start_agent_sandbox");
/** コンテナを止める(消さない)。 */
export const stopAgentSandbox = () => invoke<void>("stop_agent_sandbox");
export const resetSandbox = () => invoke<void>("reset_sandbox");
/** Dockerfileを標準の内容で上書きする。 */
export const resetSandboxDockerfile = () => invoke<void>("reset_sandbox_dockerfile");
/** `/workspace`にマウントするフォルダ(エージェントごとに決まった場所。無ければ作る)。 */
export const getSandboxWorkspaceDir = () => invoke<string>("get_sandbox_workspace_dir");
/** デバッグログの出力先を Finder で開く。 */
export const openDebugLogDir = () => invoke<void>("open_debug_log_dir");

/** `/workspace`にマウントするフォルダを Finder で開く。 */
export const openSandboxWorkspaceDir = () => invoke<void>("open_sandbox_workspace_dir");

export type SandboxProvider = "anthropic" | "bedrock";
/** コンテナ内の Claude Code の接続の設定(モデルと許可するツールは settings.json で決める)。 */
export type SandboxLlm = { provider: SandboxProvider; timeout_mins: number; idle_stop_mins: number };
/** 貼り付けた Claude Code の settings.json(未設定は空文字)。保存時に検証し、誤りがあれば保存しない。 */
/** チャットで選べるモデル(settings.json の model・availableModels)と、選ばれているモデル。 */
export type SandboxModels = { models: string[]; selected: string | null };
export const getSandboxModels = () => invoke<SandboxModels>("get_sandbox_models");
/** チャットで使うモデルを選ぶ。null で解除(settings.json の model に任せる)。 */
export const setSandboxModel = (model: string | null) => invoke<void>("set_sandbox_model", { model });
/** `own`はこのエージェントの保存(空なら全体設定を利用)、`shared`は全体設定の内容。 */
export type ClaudeSettings = { own: string; shared: string };
export const getSandboxClaudeSettings = () => invoke<ClaudeSettings>("get_sandbox_claude_settings");
/** 全体設定の settings.json。自分の settings.json を持たないエージェントが使う。 */
export const getSharedClaudeSettings = () => invoke<string>("get_shared_claude_settings");
export const setSharedClaudeSettings = (content: string) => invoke<void>("set_shared_claude_settings", { content });
export const setSandboxClaudeSettings = (content: string) => invoke<void>("set_sandbox_claude_settings", { content });
export const getSandboxLlm = () => invoke<SandboxLlm>("get_sandbox_llm");
export const setSandboxLlm = (config: SandboxLlm) => invoke<SandboxLlm>("set_sandbox_llm", { config });

/** このエージェントのDockerfile。専用の登録があればその内容、なければ共通の内容(読み取り専用)。登録があれば共通より優先する。 */
export const getAgentSandboxDockerfile = () => invoke<{ registered: boolean; content: string }>("get_agent_sandbox_dockerfile");
/** 共通のDockerfileをコピーして、専用として登録する。 */
export const registerAgentSandboxDockerfile = () => invoke<void>("register_agent_sandbox_dockerfile");
export const setAgentSandboxDockerfile = (content: string) => invoke<void>("set_agent_sandbox_dockerfile", { content });
/** 専用の登録を解除する(以後は共通を使う)。 */
export const unregisterAgentSandboxDockerfile = () => invoke<void>("unregister_agent_sandbox_dockerfile");
/** このエージェントが使うイメージ(専用の登録があれば専用)をビルドする。 */
export const buildAgentSandboxImage = (force: boolean) => invoke<void>("build_agent_sandbox_image", { force });
/** Finderでファイルを選択して開く。 */
export const revealSandboxFile = (kind: "env" | "dockerfile" | "agent_dockerfile") => invoke<void>("reveal_sandbox_file", { kind });

/** コマンド権限。各項目は`git status`のようなコマンド名と引数の接頭辞。 */
export type CommandPolicy = {
  version: number;
  allow: string[];
  ask: string[];
  deny: string[];
  deny_words: string[];
  protected: string[];
  system_paths: string[];
  system_exceptions: string[];
  confirm_overwrite: boolean;
  confirm_pipe_to_shell: boolean;
  restrict_to_knowledge: boolean;
};
export const getCommandPolicy = () => invoke<CommandPolicy>("get_command_policy");
/** 保存して、整形後(空行・重複を除いた)の内容を返す。 */
export const setCommandPolicy = (policy: CommandPolicy) => invoke<CommandPolicy>("set_command_policy", { policy });

/** 初期値のコマンド権限(保存はしない)。 */
export const getDefaultCommandPolicy = () => invoke<CommandPolicy>("get_default_command_policy");

/** デバッグモード。有効な間、API通信と文字起こしをナレッジ配下の`debug-logs`へ記録する。未設定なら無効。 */
export const getDebugMode = () => invoke<boolean>("get_debug_mode");
export const setDebugMode = (enabled: boolean) => invoke<void>("set_debug_mode", { enabled });

/** デバッグログの現在の出力先。 */
export const getDebugLogDir = () => invoke<string>("get_debug_log_dir");
/** 出力先を変更する(絶対パス)。`null`でナレッジ配下の既定に戻す。既存ログは移動しない。 */
export const setDebugLogDir = (path: string | null) => invoke<string>("set_debug_log_dir", { path });

export type DebugLogFile = { name: string; bytes: number };
/** デバッグログのファイル一覧(新しい日付が先頭)。 */
export const listDebugLogs = () => invoke<DebugLogFile[]>("list_debug_logs");
/** ログ1件の本文(JSON Lines)。 */
export const readDebugLog = (name: string) => invoke<string>("read_debug_log", { name });
/** `name`のログを削除する。省略すると全ログを削除し、削除した件数を返す。 */
export const deleteDebugLogs = (name?: string) => invoke<number>("delete_debug_logs", { name });

/** デバッグモードの切り替えをナビゲーションへ知らせるイベント名。 */
export const DEBUG_MODE_EVENT = "debug-mode-changed";

/** インターネット検索・ページ取得の可否(Claude選択時のみ有効)。未設定なら有効。 */
export const getWebAccess = () => invoke<boolean>("get_web_access");
export const setWebAccess = (enabled: boolean) => invoke<void>("set_web_access", { enabled });

/** エージェントごとの数値の上限。保存時に範囲内へ丸められる。 */
export type Tuning = {
  web_search_max: number;
  web_fetch_max: number;
  history_exchanges: number;
};
export const DEFAULT_TUNING: Tuning = {
  web_search_max: 5,
  web_fetch_max: 5,
  history_exchanges: 10,
};

/** 音声入力の待ち時間。全エージェント共通(全体設定)。保存時に範囲内へ丸められる。 */
export type VoiceTiming = {
  voice_idle_secs: number;
  utterance_silence_secs: number;
  release_tail_secs: number;
};
export const DEFAULT_VOICE_TIMING: VoiceTiming = {
  voice_idle_secs: 3,
  utterance_silence_secs: 1.2,
  release_tail_secs: 0.5,
};
export const getVoiceTiming = () => invoke<VoiceTiming>("get_voice_timing");
export const setVoiceTiming = (voice: VoiceTiming) => invoke<VoiceTiming>("set_voice_timing", { voice });

/** 全体設定のうち画面の環境の項目。未保存の項目はnull。 */
export type GlobalPrefs = { language: "ja" | "en" | null; os_notifications: boolean | null };
export const getGlobalPrefs = () => invoke<GlobalPrefs>("get_global_prefs");
export const setGlobalLanguage = (language: "ja" | "en") => invoke<void>("set_global_language", { language });
export const setGlobalOsNotifications = (enabled: boolean) => invoke<void>("set_global_os_notifications", { enabled });
/** 登録したプロキシ1件。パスワードは返さず、登録の有無だけ。 */
export type ProxyEntry = { id: string; name: string; url: string; username: string; has_password: boolean };
/** プロキシの一覧と、既定のID(なければ空)。 */
export type ProxyList = { proxies: ProxyEntry[]; default_id: string };
export const getProxies = () => invoke<ProxyList>("get_proxies");
/** 追加(idなし)または更新して、次の通信から反映する。passwordはundefinedなら据え置き、空文字なら削除。 */
export const saveProxy = (id: string | null, name: string, url: string, username: string, password?: string) =>
  invoke<ProxyList>("save_proxy", { id, name, url, username, password: password ?? null });
export const deleteProxy = (id: string) => invoke<ProxyList>("delete_proxy", { id });
/** 既定のプロキシを決める。空文字なら既定なし。 */
export const setDefaultProxy = (id: string) => invoke<ProxyList>("set_default_proxy", { id });
/** このエージェントが使うプロキシの選択。空は既定を使う、"none"は使わない、それ以外は登録済みのID。 */
export const getProxyChoice = () => invoke<string>("get_proxy_choice");
export const setProxyChoice = (choice: string) => invoke<void>("set_proxy_choice", { choice });
export const getTuning = () => invoke<Tuning>("get_tuning");
/** 保存し、範囲内へ丸めた実際の値を返す。 */
export const setTuning = (tuning: Tuning) => invoke<Tuning>("set_tuning", { tuning });

/** LLMへ常に先頭で渡すベース指示。未登録なら既定値が返る。空文字で保存すると既定値に戻る。 */
export const getBaseInstructions = () => invoke<string>("get_base_instructions");
export const saveBaseInstructions = (text: string) => invoke<void>("save_base_instructions", { text });

/** 設定ページで手動入力するパーソナルデータ。空の項目はnull。無効の間はAIへ何も渡さない。 */
export type UserProfile = {
  enabled: boolean;
  name: string | null;
  nickname: string | null;
  /** YYYY-MM-DD。AIへは年齢だけを渡す。 */
  birthday: string | null;
  location: string | null;
  /** IANA名(例: Asia/Tokyo)。 */
  timezone: string | null;
  /** 言語コード(例: ja、en-US)。 */
  language: string | null;
};
export const getProfile = () => invoke<UserProfile>("get_profile");
/** 検証して保存し、整えた後の値を返す。 */
export const saveProfile = (profile: UserProfile) => invoke<UserProfile>("save_profile", { profile });
export const clearProfile = () => invoke<void>("clear_profile");

/** エージェントデータ・共通データ。ユーザーが書いた自由記述の指示で、有効な間だけAIへ毎回渡す。 */
export type AgentNote = { enabled: boolean; text: string };
export const AGENT_NOTE_MAX_CHARS = 2000;
export const getAgentData = () => invoke<AgentNote>("get_agent_data");
/** 検証して保存し、整えた後の値を返す。エージェントの画面でだけ使う。 */
export const saveAgentData = (note: AgentNote) => invoke<AgentNote>("save_agent_data", { note });
export const getSharedData = () => invoke<AgentNote>("get_shared_data");
export const saveSharedData = (note: AgentNote) => invoke<AgentNote>("save_shared_data", { note });

export type SttEngine = "local" | "google";
export const getSttEngine = () => invoke<SttEngine>("get_stt_engine");
export const setSttEngine = (engine: SttEngine) => invoke<void>("set_stt_engine", { engine });
/** 音声認識の言語。autoは自動判定。 */
export type SttLanguage = "auto" | "ja" | "en" | "zh" | "ko";
export const getSttLanguage = () => invoke<SttLanguage>("get_stt_language");
export const setSttLanguage = (language: SttLanguage) => invoke<void>("set_stt_language", { language });
export const sttModelReady = () => invoke<boolean>("stt_model_ready");
/** 高精度モデルへ更新できるか(旧モデルだけインストール済み)。 */
export const sttModelUpgradable = () => invoke<boolean>("stt_model_upgradable");
export const downloadSttModel = () => invoke<void>("download_stt_model");
/** 音声認識モデルの取得中の進み具合(取得中でなければnull)。 */
export const sttDownloadProgress = () => invoke<DownloadProgress | null>("stt_download_progress");
export const startRecording = () => invoke<void>("start_recording");
/** 録音を文字起こしせずに破棄する。 */
export const cancelRecording = () => invoke<void>("cancel_recording");
/** 録音中のマイク入力の直近波形(-1〜1)。録音中でなければ空配列。 */
export const recordingWaveform = () => invoke<number[]>("recording_waveform");
/**
 * 校正モデルが登録されていれば、sessionIdの直近の会話を文脈に校正した文字列を返す。
 * answeringが真(承認への返答)なら、語彙ヒントに「はい」「いいえ」を加え、校正は省く。
 */
export const stopRecordingAndTranscribe = (sessionId?: string, answering = false) =>
  invoke<string>("stop_recording_and_transcribe", { sessionId, answering });
/** 常時待機の判定結果。起動ワードを含まない発話は校正(LLM)に回らず、`woke`がfalseで返る。 */
export type WakeCheck = { woke: boolean; text: string };
/** 録音を止めて文字起こしし、起動ワードを含むか判定する。`text`は起動ワードに続く依頼(校正済み。なければ空)。 */
export const stopRecordingAndCheckWake = (sessionId?: string) =>
  invoke<WakeCheck>("stop_recording_and_check_wake", { sessionId });
/** 起動ワード登録用に、録音を止めて文字起こしした生の認識結果を返す(判定・校正なし)。 */
export const transcribeWakeSample = () => invoke<string>("transcribe_wake_sample");
/** 直近`keepMs`ミリ秒の入力だけを残して録音バッファを切り詰める(常時待機用)。 */
export const trimRecording = (keepMs: number) => invoke<void>("trim_recording", { keepMs });

/** 常時音声待機の設定。起動ワードを含む発話があったときだけ依頼の待ち受けに入る。 */
export type WakeConfig = { enabled: boolean; words: string[] };
export const getWakeConfig = () => invoke<WakeConfig>("get_wake_config");
/** 保存し、整形後の実際の設定を返す(短すぎる・重複する語は除かれ、語がなければ無効になる)。 */
export const setWakeConfig = (config: WakeConfig) => invoke<WakeConfig>("set_wake_config", { config });

/** 繰り返し使う定型音声の作成状況。 */
export type VoicePhrase = { id: string; text: string; ready: boolean };
export const voicePhrasesStatus = () => invoke<VoicePhrase[]>("voice_phrases_status");
/** 未作成の定型音声を現在の読み上げエンジンで合成して保存する(作成済みは送らない)。 */
export const generateVoicePhrases = () => invoke<VoicePhrase[]>("generate_voice_phrases");
/** 作成済みの定型音声(base64のMP3またはWAV)。未作成ならnullで、TTSへは送らない。`wake_ack`=起動後の応答、`switch_confirm`=会話切り替えの確認。 */
export const voicePhraseAudio = (id: "wake_ack" | "switch_confirm") =>
  invoke<string | null>("voice_phrase_audio", { id });

/** 文字起こしの校正に使うモデル。未登録(校正オフ)ならnull。 */
export const loadProofreadRoute = () => invoke<Route | null>("load_proofread_route");
/** 校正モデルを登録する。nullで校正をオフにする。 */
export const saveProofreadRoute = (route: Route | null) => invoke<void>("save_proofread_route", { route });
/** 現在の読み上げエンジンで音声化し、base64(Google CloudはMP3、VOICEVOXはWAV)を返す。 */
export const synthesizeSpeech = (text: string) => invoke<string>("synthesize_speech", { text });

/** 読み上げエンジン。VOICEVOXはローカルのVOICEVOX CORE(キー不要・無料)。 */
export type TtsEngine = "google" | "voicevox";
export const getTtsEngine = () => invoke<TtsEngine>("get_tts_engine");
export const setTtsEngine = (engine: TtsEngine) => invoke<void>("set_tts_engine", { engine });
/** 選んだエンジンで読み上げられる状態か(Google Cloudはキー登録済み、VOICEVOXはCOREを取得済み)。 */
export const ttsReady = () => invoke<boolean>("tts_ready");

export type VoicevoxSpeaker = { name: string; styles: { id: number; name: string }[] };
export const voicevoxInstalled = () => invoke<boolean>("voicevox_installed");
/** VOICEVOX COREと辞書・音声モデルを取得する(約90MB)。取得済みでも取得し直す。 */
export const installVoicevox = () => invoke<void>("install_voicevox");
/** VOICEVOXの取得中の進み具合(取得中でなければnull)。 */
export const voicevoxDownloadProgress = () => invoke<DownloadProgress | null>("voicevox_download_progress");
/** 取得済みの音声モデルの話者とスタイル。 */
export const voicevoxSpeakers = () => invoke<VoicevoxSpeaker[]>("voicevox_speakers");
export const getVoicevoxStyle = () => invoke<number>("get_voicevox_style");
export const setVoicevoxStyle = (style: number) => invoke<void>("set_voicevox_style", { style });

/** VOICEVOXの配布物のバージョン。取得元のホストは固定で、識別子だけを変えられる。 */
export type VoicevoxVersions = { core: string; onnxruntime: string; models: string; dict_release: string; dict: string };
/** ローカル音声(WhisperとVOICEVOX)の取得バージョン。`*_installed`は取得済みのもの。 */
export type ModelSources = {
  whisper_revision: string;
  whisper_default: string;
  voicevox: VoicevoxVersions;
  voicevox_installed: VoicevoxVersions | null;
  voicevox_default: VoicevoxVersions;
};
export const getModelSources = () => invoke<ModelSources>("get_model_sources");
/** 次回の取得に使うWhisperモデルのリビジョン(ブランチ・タグ・コミット)を保存する。 */
export const setWhisperRevision = (revision: string) => invoke<void>("set_whisper_revision", { revision });
/** 次回の取得に使うVOICEVOXのバージョンを保存する。取得済みのものは取得し直すまで変わらない。 */
export const setVoicevoxVersions = (versions: VoicevoxVersions) => invoke<void>("set_voicevox_versions", { versions });

/** 読み上げ前に置き換える語と読み。組み込み(読み取り専用)と、ユーザーが登録した分。 */
export type DictionaryEntry = { word: string; reading: string };
export type TtsDictionary = { builtin: DictionaryEntry[]; user: DictionaryEntry[] };
export const getTtsDictionary = () => invoke<TtsDictionary>("get_tts_dictionary");
/** ユーザー辞書を丸ごと保存し、整形後(空欄・大文字小文字違いの重複を除いた)の内容を返す。 */
export const setTtsDictionary = (entries: DictionaryEntry[]) => invoke<DictionaryEntry[]>("set_tts_dictionary", { entries });
export type DictionaryImportReport = { entries: DictionaryEntry[]; added: number; updated: number };
/** ユーザー辞書をJSONファイルへ書き出し、書き出した語数を返す。 */
export const exportTtsDictionary = (dest: string) => invoke<number>("export_tts_dictionary", { dest });
/** JSONファイルの語をユーザー辞書へ取り込む。重複する語は取り込み側で上書きする。 */
export const importTtsDictionary = (src: string) => invoke<DictionaryImportReport>("import_tts_dictionary", { src });

export const ttsKeyRegistered = () => invoke<boolean>("tts_key_registered");
export const saveTtsKey = (key: string) => invoke<void>("save_tts_key", { key });
export const deleteTtsKey = () => invoke<void>("delete_tts_key");


/** 通信系の障害はUI表示に加えてローカルのビープ音でも通知する。 */
const AUDIBLE_ERRORS = new Set([
  "timeout",
  "rate_limited",
  "server_error",
  "network_error",
  "unauthorized",
]);

let errorSound: HTMLAudioElement | null = null;

export function playErrorSound() {
  try {
    errorSound ??= new Audio("/sounds/error.wav");
    errorSound.currentTime = 0;
    void errorSound.play().catch(() => {});
  } catch {
    // 通知音の失敗でエラー処理自体を壊さない
  }
}

let audioCtx: AudioContext | null = null;
let currentSource: AudioBufferSourceNode | null = null;
let outputAnalyser: AnalyserNode | null = null;
/** AudioContextが使えないときのフォールバック再生。 */
let currentAudio: HTMLAudioElement | null = null;
let speaking = false;
/** 再生中の読み上げの情報。字幕を再生位置に合わせるために使う。`text`は読み上げている本文。 */
let playback: { text: string; startedAt: number; duration: number } | null = null;

/** 読み上げ音声を再生中か。 */
export const isSpeaking = () => speaking;

/** `text`を読み上げ中なら、再生の進行度(0〜1)。それ以外はnull。 */
export function speechProgress(text: string): number | null {
  if (!speaking || !playback || playback.text !== text) return null;
  const now = currentAudio ? currentAudio.currentTime : audioCtx?.currentTime;
  if (now === undefined || !(playback.duration > 0)) return null;
  return Math.min(1, Math.max(0, (now - playback.startedAt) / playback.duration));
}

/** 再生中の読み上げが終わるまで待つ(上限`maxMs`)。長い返答の途中でマイクが開かないよう、上限は十分長くしてある。 */
export async function waitForSpeechEnd(maxMs = 300000): Promise<void> {
  const until = Date.now() + maxMs;
  while (speaking && Date.now() < until) await new Promise((r) => setTimeout(r, 50));
}

/** 読み上げ音声の波形取得用。再生がなければ無音を返す。AudioContextが使えなければnull。 */
export function getOutputAnalyser(): AnalyserNode | null {
  if (!audioCtx) return null;
  if (!outputAnalyser) {
    outputAnalyser = audioCtx.createAnalyser();
    outputAnalyser.fftSize = 1024;
    outputAnalyser.connect(audioCtx.destination);
  }
  return outputAnalyser;
}

/**
 * 自動再生のためにAudioContextを有効化する。
 * ユーザー操作(送信・マイク操作)の中で呼んでおくと、以降の応答は操作なしで再生できる。
 */
export function unlockAudio() {
  try {
    installAudioRecovery();
    audioCtx ??= new AudioContext();
    if (audioCtx.state !== "running") void audioCtx.resume().catch(() => {});
  } catch {
    // 非対応環境ではHTMLAudioへフォールバックする
  }
}

let recoveryInstalled = false;

/**
 * 放置やスリープで中断されたAudioContextを、アプリが前面に戻ったときに復旧する。
 * 中断(suspended/interrupted)のまま放置されると、次の応答が無音になるため。
 */
function installAudioRecovery() {
  if (recoveryInstalled || typeof window === "undefined") return;
  recoveryInstalled = true;
  const recover = () => {
    if (document.visibilityState === "hidden" || !audioCtx || audioCtx.state === "running" || audioCtx.state === "closed") return;
    void audioCtx.resume().catch(() => {});
  };
  document.addEventListener("visibilitychange", recover);
  window.addEventListener("focus", recover);
}

/** 再生終了通知が来なかった場合に再生中扱いを解除するまでの余裕(ms)。 */
const SPEAKING_GRACE_MS = 2000;
let speakingTimer: ReturnType<typeof setTimeout> | null = null;

const clearSpeakingTimer = () => {
  if (speakingTimer) clearTimeout(speakingTimer);
  speakingTimer = null;
};

/** AudioContextを破棄する。長時間の放置などで復帰できなくなった場合に作り直すための準備。 */
function resetAudio() {
  clearSpeakingTimer();
  speaking = false;
  playback = null;
  currentSource = null;
  outputAnalyser = null;
  const old = audioCtx;
  audioCtx = null;
  void old?.close().catch(() => {});
}

/** 再生可能な状態のAudioContextを返す。停止したまま戻らなければ作り直し、それでも駄目なら例外。 */
async function runningAudioContext(): Promise<AudioContext> {
  const tryResume = async () => {
    unlockAudio();
    if (audioCtx && audioCtx.state !== "running") {
      await Promise.race([audioCtx.resume().catch(() => {}), new Promise((r) => setTimeout(r, 1000))]);
    }
    return audioCtx?.state === "running";
  };
  // 放置後はsuspended/interruptedのまま戻らないことがあるため、1度だけ作り直して再試行する
  if (!(await tryResume())) {
    resetAudio();
    if (!(await tryResume())) throw new Error("AudioContext is not running");
  }
  return audioCtx!;
}

/** 再生中の読み上げを止める。 */
export function stopSpeech(): void {
  try {
    currentSource?.stop();
  } catch {
    // すでに停止済み
  }
  if (currentAudio) {
    const audio = currentAudio;
    currentAudio = null;
    audio.pause();
  }
  clearSpeakingTimer();
  speaking = false;
  playback = null;
}

/** AudioContextを使えないときに、HTMLAudioで再生する。 */
async function playWithHtmlAudio(bytes: Uint8Array, text?: string): Promise<void> {
  const riff = bytes[0] === 0x52 && bytes[1] === 0x49; // VOICEVOXのWAVは"RIFF"で始まる
  const url = URL.createObjectURL(new Blob([bytes as BlobPart], { type: riff ? "audio/wav" : "audio/mpeg" }));
  const audio = new Audio(url);
  const finish = () => {
    URL.revokeObjectURL(url);
    if (currentAudio !== audio) return;
    currentAudio = null;
    clearSpeakingTimer();
    speaking = false;
    playback = null;
  };
  audio.onended = finish;
  audio.onerror = finish;
  currentAudio = audio;
  try {
    await audio.play();
  } catch (e) {
    finish();
    throw e;
  }
  if (currentAudio !== audio) return; // 再生を始める間に止められた
  speaking = true;
  const duration = Number.isFinite(audio.duration) ? audio.duration : 0;
  playback = text ? { text, startedAt: 0, duration } : null;
  if (duration > 0) speakingTimer = setTimeout(finish, duration * 1000 + SPEAKING_GRACE_MS);
}

/**
 * 再生を開始する。直前の再生は止める。再生できない場合は`playback_error`で失敗する。
 * `text`は読み上げる本文。渡すと`speechProgress`で再生位置を取れる(定型音声など本文がないものは省く)。
 */
export async function playBase64Mp3(b64: string, text?: string): Promise<void> {
  stopSpeech();
  try {
    const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    // 操作なしで再生を始めると(常時待機の応答など)中断状態のままになり、終了通知が来ず再生中扱いが残る。
    // 復旧できなければHTMLAudioで再生し、それも駄目なら失敗にする
    let ctx: AudioContext;
    try {
      ctx = await runningAudioContext();
    } catch {
      await playWithHtmlAudio(bytes, text);
      return;
    }
    const buffer = await ctx.decodeAudioData(bytes.buffer);
    const source = ctx.createBufferSource();
    source.buffer = buffer;
    source.connect(getOutputAnalyser() ?? ctx.destination);
    const finish = () => {
      if (currentSource !== source) return;
      clearSpeakingTimer();
      speaking = false;
      playback = null;
    };
    source.onended = finish;
    source.start();
    currentSource = source;
    speaking = true;
    playback = text ? { text, startedAt: ctx.currentTime, duration: buffer.duration } : null;
    // 再生中にコンテキストが中断されるとonendedが来ないため、再生時間を過ぎたら再生中扱いを解除する
    speakingTimer = setTimeout(finish, buffer.duration * 1000 + SPEAKING_GRACE_MS);
  } catch (e) {
    throw { code: "playback_error", message: String(e) } satisfies AppError;
  }
}

/** メッセージを返しつつ、必要なら通知音を鳴らす。 */
export function reportError(e: unknown): string {
  const code = (e as Partial<AppError>)?.code;
  if (code && AUDIBLE_ERRORS.has(code)) playErrorSound();
  return errorMessage(e);
}

export type KnowledgeFile = {
  path: string;
  title: string;
  category: string;
  updated_at: string;
};
export type BatchReport = {
  processed_logs: number;
  files_written: number;
  failed_segments: number;
  captures_removed: number;
};

/** 1エージェント分のナレッジ(全エージェント一覧用)。`error`があれば一覧を取れなかった。`color`はパーソナルでは null。 */
export type AgentKnowledge = {
  id: string;
  name: string;
  color: string | null;
  root: string;
  files: KnowledgeFile[];
  dirs: string[];
  error: string | null;
};
/** パーソナル・リーダー・全エージェントのナレッジ(切り替えずに操作できる)。 */
export const listAllKnowledge = () => invoke<AgentKnowledge[]>("list_all_knowledge");
export const readKnowledgeIn = (agentId: string, path: string) => invoke<string>("read_knowledge_in", { agentId, path });
export const writeKnowledgeIn = (agentId: string, path: string, content: string) =>
  invoke<void>("write_knowledge_in", { agentId, path, content });
export const createKnowledgeFileIn = (agentId: string, path: string) =>
  invoke<string>("create_knowledge_file_in", { agentId, path });
export const createKnowledgeDirIn = (agentId: string, path: string) =>
  invoke<void>("create_knowledge_dir_in", { agentId, path });
export const renameKnowledgeIn = (agentId: string, from: string, to: string) =>
  invoke<void>("rename_knowledge_in", { agentId, from, to });
export const deleteKnowledgeIn = (agentId: string, path: string) =>
  invoke<void>("delete_knowledge_in", { agentId, path });
/** Finderで開く。`path`ありならそれを選択した状態で、なしならナレッジフォルダを開く。 */
export const revealKnowledgeInFinder = (agentId: string, path?: string) =>
  invoke<void>("reveal_knowledge_in_finder", { agentId, path: path ?? null });

/** ナレッジ(Markdown)の保存先フォルダ。Obsidianのvaultとして開ける。 */
export const knowledgeDir = () => invoke<string>("knowledge_dir");
/** 保存先を変更する(nullで既定に戻す)。既存ファイルは移動されない。変更後のパスを返す。 */
/** ナレッジの保存先を Finder で開く。 */
export const openKnowledgeDir = () => invoke<void>("open_knowledge_dir");
export const setKnowledgeDir = (path: string | null) => invoke<string>("set_knowledge_dir", { path });
/** スキル(決まった依頼への対応手順)。ナレッジ配下の`.my-assistant/skills/<name>.md`に、エージェントごとに保存される。 */
export type Skill = { name: string; description: string; body: string };
/** スキルの一覧と、選定で毎回送る前置き(指示 + 全スキルの名前・説明)の概算トークン数。 */
export type SkillsOverview = { skills: Skill[]; index_tokens: number };
export const SKILL_LIMITS = { name: 64, description: 1024, body: 3000 } as const;
/** 名前に使える文字(英数字・日本語・`_`・`-`)。ファイル名になるため`.`や区切り文字は不可。 */
export const SKILL_NAME_RE = /^[a-z0-9]+(-[a-z0-9]+)*$/;

export const listSkills = () => invoke<SkillsOverview>("list_skills");
export const createSkill = (skill: Skill) => invoke<Skill>("create_skill", { skill });
export const updateSkill = (skill: Skill) => invoke<Skill>("update_skill", { skill });
export const deleteSkill = (name: string) => invoke<void>("delete_skill", { name });
export type SkillImportPreview = { total: number; conflicts: string[] };
export type SkillImportReport = { added: number; updated: number; skipped: number };
/** 全スキルをJSONファイルへ書き出す。件数を返す。 */
export const exportSkills = (dest: string) => invoke<number>("export_skills", { dest });
/** 取り込み前の確認(件数と、上書きになる名前)。何も変更しない。 */
export const previewImportSkills = (src: string) => invoke<SkillImportPreview>("preview_import_skills", { src });
export const importSkills = (src: string, overwrite: boolean) => invoke<SkillImportReport>("import_skills", { src, overwrite });
/** 選定に使うモデル。未設定なら既定(Haiku)。使えるモデルが無ければ`null`。 */
export const loadSkillsRoute = () => invoke<Route | null>("load_skills_route");
export const saveSkillsRoute = (route: Route) => invoke<void>("save_skills_route", { route });


export const loadBatchRoute = () => invoke<Route | null>("load_batch_route");
export const saveBatchRoute = (route: Route) => invoke<void>("save_batch_route", { route });
export const listKnowledge = () => invoke<KnowledgeFile[]>("list_knowledge");
export const readKnowledge = (path: string) => invoke<string>("read_knowledge", { path });
/** Markdown以外のファイル一覧(閲覧専用)。 */
export const listOtherKnowledge = () => invoke<KnowledgeFile[]>("list_other_knowledge");
export const readOtherKnowledge = (path: string) => invoke<string>("read_other_knowledge", { path });
export const listKnowledgeDirs = () => invoke<string[]>("list_knowledge_dirs");
export const writeKnowledge = (path: string, content: string) => invoke<void>("write_knowledge", { path, content });
export const createKnowledgeFile = (path: string) => invoke<string>("create_knowledge_file", { path });
export const createKnowledgeDir = (path: string) => invoke<void>("create_knowledge_dir", { path });
export const renameKnowledge = (from: string, to: string) => invoke<void>("rename_knowledge", { from, to });
export const deleteKnowledge = (path: string) => invoke<void>("delete_knowledge", { path });
export type TransferReport = { files: number; skipped: number; dest: string | null };
/** ナレッジ全体を選んだフォルダ直下の新しいフォルダへ書き出す。 */
export const exportKnowledge = (dest: string) => invoke<TransferReport>("export_knowledge", { dest });
/** 選んだフォルダのMarkdownを取り込む。既存ファイルは上書きしない。 */
export const importKnowledge = (src: string) => invoke<TransferReport>("import_knowledge", { src });
/** ナレッジのMarkdownをすべて削除する(元に戻せない)。 */
export const resetKnowledge = () => invoke<TransferReport>("reset_knowledge");
/** 実行中・対象ログなし・キー未登録の場合は null。 */
export const runBatch = () => invoke<BatchReport | null>("run_batch");

/** 覚えておくこと1件。 */
export type MemoryPin = { text: string; date: string };
/** エージェントの記憶(覚えておくこと)。 */
export type MemoryView = { pins: MemoryPin[] };
/** 記憶のファイル(ナレッジ内の相対パス)。編集は通常のノートと同じ画面で行う。 */
export const MEMORY_PINNED_PATH = "Memory/pinned.md";
export const memoryView = () => invoke<MemoryView>("memory_view");
export const memoryForget = (text: string) => invoke<boolean>("memory_forget", { text });

export type SettingsTransferReport = { applied: number; skipped: number };
/** 設定をJSONファイルへ書き出す。APIキーは含まれない。 */
export const exportSettings = (dest: string) => invoke<SettingsTransferReport>("export_settings", { dest });
/** 書き出した設定ファイルを読み込んで反映する。 */
export const importSettings = (src: string) => invoke<SettingsTransferReport>("import_settings", { src });

export type BackupSummary = { workspaces: number; files: number; bytes: number };
export type RestorePreview = { created_at: string; workspaces: number; files: number };
/** アプリ全体(全エージェントのDB・ナレッジ・APIキー)をzipへ書き出す。パスワードがあればAES-256で暗号化する。 */
export const backupCreate = (dest: string, password: string | null) => invoke<BackupSummary>("backup_create", { dest, password });
/** バックアップのzipを検証して展開する。現在のデータは変えない。暗号化されていればパスワードが要る。 */
export const restorePrepare = (src: string, password: string | null) => invoke<RestorePreview>("restore_prepare", { src, password });
/** 展開済みのバックアップで、アプリを再起動して差し替える。 */
export const restoreApply = () => invoke<void>("restore_apply");
/** 展開済みのバックアップを捨てる。 */
export const restoreDiscard = () => invoke<void>("restore_discard");

/** トレイメニューの表示言語を切り替える。 */

/** パーソナルワークスペースのID。 */
export const PERSONAL_ID = "personal";
/** リーダー(各エージェントのタスク進捗を管理する特別なエージェント)のID・表示名・イメージカラー。エージェントの一覧には入らない。 */
export const LEADER_ID = "leader";
export const LEADER_NAME = "Leader";
export const LEADER_COLOR = "#f59e0b";
/** 作れるエージェントの上限。 */
export const MAX_AGENTS = 30;
/** エージェント名の最大文字数。 */
export const AGENT_NAME_MAX = 40;

/** ワークの1エージェント(ワーキングエージェント)。`color`は`#rrggbb`。 */
export type Agent = { id: string; name: string; color: string; created_at: string };
/** 現在のワークスペース。`agent`はパーソナルのとき`null`。 */
export type ActiveWorkspace = { id: string; agent: Agent | null };

export const WORKSPACE_CHANGED_EVENT = "workspace-changed";

/** 開いていないエージェントで、対応が必要になったときの通知。 */
export const TASK_ATTENTION_EVENT = "task-attention";
export type TaskAttention = {
  /** 対象のワークスペース(`personal`またはエージェントID)。 */
  workspace_id: string;
  /** エージェントの名称とイメージカラー。パーソナルは`null`。 */
  workspace_name: string | null;
  workspace_color: string | null;
  /** approval=承認が必要、finished=依頼が完了、failed=依頼が失敗。 */
  kind: "approval" | "finished" | "failed";
  /** 承認の種類。承認のときだけ。 */
  approval_kind: ApprovalKind | null;
  /** 承認を求められている内容(ファイルは作成先、コマンドはその内容)。計画では空。 */
  detail: string | null;
  error: AppError | null;
};

/** エージェントに選べるイメージカラー(30色)。 */
export const listAgentColors = () => invoke<string[]>("list_agent_colors");
export const listAgents = () => invoke<Agent[]>("list_agents");
export const getActiveWorkspace = () => invoke<ActiveWorkspace>("get_active_workspace");
export const createAgent = (name: string, color: string) => invoke<Agent>("create_agent", { name, color });
export const updateAgent = (id: string, name: string, color: string) =>
  invoke<Agent>("update_agent", { id, name, color });
/** 削除の結果。`keptKnowledge`は、残したナレッジの場所(残さなかった、または無かったときは null)。 */
export type DeleteReport = { active: ActiveWorkspace; keptKnowledge: string | null };
/** エージェントを削除する。`deleteKnowledge`が false なら、既定の場所のナレッジは消さずに残す。 */
export const deleteAgent = (id: string, deleteKnowledge: boolean) =>
  invoke<DeleteReport>("delete_agent", { id, deleteKnowledge });
/** コピーした設定の項目数と、コピーしたキーの件数(キーの値は返らない)。 */
export type CopyReport = { settings: number; keys: number; words: number };
/** `from`の設定を`to`へコピーする(`personal`またはエージェントID)。コピー先の既存の設定は上書きされる。 */
export const copyWorkspaceSettings = (from: string, to: string, includeKeys: boolean, includeDictionary: boolean) =>
  invoke<CopyReport>("copy_workspace_settings", { from, to, includeKeys, includeDictionary });
export const switchWorkspace = (id: string) => invoke<ActiveWorkspace>("switch_workspace", { id });

// ---- リーダー(各エージェントのタスク進捗を管理する特別なエージェント) ----

/** 依頼のいまの状態。`interrupted`は、再起動などで依頼が失われたもの。 */
export type TaskStatus = "running" | "awaiting_approval" | "done" | "failed" | "interrupted";
/** 台帳の1行(1回の配布の、1つの宛先)。結果の本文は持たない。 */
export type LeaderTask = {
  id: string;
  batch_id: string;
  instruction: string;
  agent_id: string;
  /** 配布したときの宛先の名称(エージェントが削除されても残る)。 */
  agent_name: string;
  status: string;
  confirmed: boolean;
  created_at: string;
  live_status: TaskStatus;
  agent_exists: boolean;
  agent_color: string | null;
};
/** 各エージェントのいまの依頼の状態(`idle`=動いていない)。直接の指示によるものも含む。 */
export type AgentRun = { id: string; name: string; color: string; status: "idle" | "running" | "awaiting_approval" | "done" | "failed" };
/** `limit`=同時に指示できるエージェント数の上限、`active`=リーダーが配布していま動いている(実行中・承認待ち)エージェントの数。 */
export type LeaderOverview = { tasks: LeaderTask[]; agents: AgentRun[]; limit: number; active: number };
/** エージェント数の上限(バックエンドの`MAX_AGENTS`と同じ)。同時に指示できる数の上限の最大値。 */
export const LEADER_LIMIT_MAX = 30;
/** 配布できなかった理由。`active`=選択中のエージェント、`limit`=同時に指示できる数の上限、`busy`=依頼の最中。 */
export type DispatchReason = "not_found" | "active" | "limit" | "busy" | "no_provider" | "error";
export type DispatchResult = { agent_id: string; started: boolean; reason: DispatchReason | null };

export const leaderExists = () => invoke<boolean>("leader_exists");
/** ワークが有効か(未保存は有効)。無効のあいだも、リーダー・エージェントのデータは残る。 */
export const workEnabled = () => invoke<boolean>("work_enabled");
/** ワークの有効/無効を切り替える。実行中・承認待ちがあるときは無効にできない。 */
export const setWorkEnabled = (enabled: boolean) => invoke<void>("set_work_enabled", { enabled });
export const createLeader = () => invoke<void>("create_leader");
/** リーダープロンプト(行動原理・役割)。 */
export const leaderGetPrompt = () => invoke<AgentNote>("leader_get_prompt");
export const leaderSetPrompt = (note: AgentNote) => invoke<AgentNote>("leader_set_prompt", { note });
/** リーダーのAPIキーの登録有無のみ(生値は返らない)。 */
export const leaderGetLimit = () => invoke<number>("leader_get_limit");
export const leaderSetLimit = (limit: number) => invoke<number>("leader_set_limit", { limit });
export const leaderKeyStatus = () => invoke<KeyStatus>("leader_key_status");
export const leaderSaveApiKey = (provider: Provider, key: string) => invoke<void>("leader_save_api_key", { provider, key });
export const leaderDeleteApiKey = (provider: Provider) => invoke<void>("leader_delete_api_key", { provider });
/** `from`(`personal`またはエージェントID)のAnthropicのAPIキーを、リーダーへコピーする。コピーできたか(コピー元に無ければ false)。 */
export const leaderCopyApiKey = (from: string) => invoke<boolean>("leader_copy_api_key", { from });
/** 宛先の提案(エージェントID)。過去に配布した指示の履歴だけが根拠で、根拠が無ければ空。 */
/** 宛先の提案に使うモデル(自動選択)。キーが無ければnull。 */
export const loadSuggestRoute = () => invoke<Route | null>("load_suggest_route");
export const leaderSuggest = (text: string) => invoke<string[]>("leader_suggest", { text });
/** 指示を、選んだエージェントへ配布する。宛先は必ず利用者が選ぶ。 */
export const leaderDispatch = (text: string, agentIds: string[]) =>
  invoke<DispatchResult[]>("leader_dispatch", { text, agentIds });
export const leaderOverview = () => invoke<LeaderOverview>("leader_overview");
export const leaderSetConfirmed = (id: string, confirmed: boolean) => invoke<void>("leader_set_confirmed", { id, confirmed });

/** LLMへ渡すプロンプトを持つ機能。 */
export type PromptId = "chat" | "summary" | "proofread" | "organize" | "skills" | "suggest";
export type PromptInfo = {
  id: PromptId;
  /** いま使われる文章。 */
  text: string;
  /** 「全体設定を利用」で使われる文章(全体設定にあればそれ、なければ組み込みの既定)。 */
  default: string;
  /** このエージェントで上書きしている(個別設定)か。falseなら全体設定を利用している。 */
  custom: boolean;
};
export const PROMPT_MAX_CHARS = 20000;
export const listPrompts = () => invoke<PromptInfo[]>("list_prompts");
/** 空、または既定と同じ文章は上書きなしになる。保存後の状態を返す。 */
export const savePrompt = (id: PromptId, text: string) => invoke<PromptInfo>("save_prompt", { id, text });
/** 上書きを外し、全体設定を利用する状態へ戻す。 */
export const resetPrompt = (id: PromptId) => invoke<PromptInfo>("reset_prompt", { id });
/** 全体設定のプロンプト。`custom`は組み込みの既定から書き換えているか、`default`は組み込みの既定。 */
export const listGlobalPrompts = () => invoke<PromptInfo[]>("list_global_prompts");
export const saveGlobalPrompt = (id: PromptId, text: string) => invoke<PromptInfo>("save_global_prompt", { id, text });
export const resetGlobalPrompt = (id: PromptId) => invoke<PromptInfo>("reset_global_prompt", { id });

/** 会話の継続。保存済みの現在の会話のIDと、画面に戻す直近のやり取り(発話ごとの実行ログ付き)。 */
export type SessionTurn = {
  user: string;
  user_at: string;
  reply: string | null;
  reply_at: string | null;
  via: string | null;
  /** 実行ログ(`TaskEvent`のうち、呼び出し・結果・コマンド出力)。 */
  events: TaskEvent[];
};
export type ChatSessionView = { session_id: string; turns: SessionTurn[] };
export const chatSessionLoad = () => invoke<ChatSessionView>("chat_session_load");
/** 新しい会話のIDを保存する。次回の起動から、その会話を再開する。 */
export const chatSessionNew = (sessionId: string) => invoke<void>("chat_session_new", { sessionId });
