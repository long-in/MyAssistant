import { Channel, invoke } from "@tauri-apps/api/core";
import { tr } from "@/lib/i18n";

export const PROVIDERS = ["anthropic", "openai"] as const;
export type Provider = (typeof PROVIDERS)[number];
export type KeyStatus = Record<Provider, boolean>;

export type AppError = { code: string; message: string; source?: string | null };

/** エラーの発生元APIの表示名。 */
export const SOURCE_LABELS: Record<string, string> = {
  anthropic: "Anthropic (Claude)",
  openai: "OpenAI (GPT)",
  google_tts: "Google Cloud Text-to-Speech",
  google_stt: "Google Cloud Speech-to-Text",
  aqua_stt: "Aqua Voice",
  voicevox: "VOICEVOX",
};

export const PROVIDER_LABELS: Record<Provider, string> = {
  anthropic: "Anthropic (Claude)",
  openai: "OpenAI (GPT)",
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
  invalid_version: "バージョンには英数字と . _ - のみ使えます(64文字まで)。",
  path_not_allowed: "ナレッジディレクトリ外のパスにはアクセスできません。",
  io_error: "ファイルの読み書きに失敗しました。",
  invalid_settings: "設定ファイルの形式が正しくありません。このアプリが書き出したファイルを選んでください。",
};

export function errorMessage(e: unknown): string {
  const { code, message, source } = (e ?? {}) as Partial<AppError>;
  const body =
    code === "provider_rejected" && message
      ? tr("拒否されました: {message}", { message })
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

export type Route = { provider: Provider; model: string };

export type ModelInfo = {
  provider: Provider;
  id: string;
  label: string;
  tier: "light" | "heavy";
};

export type ChatReply = { content: string; provider: Provider; model: string };

export type UsageRow = {
  provider: Provider;
  model: string;
  requests: number;
  input_tokens: number;
  output_tokens: number;
};
/** 聞き取り間違いの訂正をナレッジ(Markdown)へ記録する。保存した場合はtrue。 */
export const addSttCorrection = (heard: string, correct: string) =>
  invoke<boolean>("add_stt_correction", { heard, correct });
/** 当月のトークン使用量(このアプリ経由の呼び出しのローカル集計)。 */
export const getUsageThisMonth = () => invoke<UsageRow[]>("get_usage_this_month");

export const listModels = () => invoke<ModelInfo[]>("list_models");
export const loadRouting = () => invoke<Route | null>("load_routing");
export const saveRouting = (route: Route) => invoke<void>("save_routing", { route });
/** Claudeを選んでいるときはナレッジのファイル操作も行い、その進捗を`onEvent`へ流す。 */
export const sendChat = (sessionId: string, text: string, route: Route, onEvent: (e: AgentEvent) => void) => {
  const channel = new Channel<AgentEvent>();
  channel.onmessage = onEvent;
  return invoke<ChatReply>("send_chat", { sessionId, text, route, onEvent: channel });
};

/** 承認の種類。file=ファイル作成、plan=作業計画、command=計画外・破壊的なコマンドの個別承認、allow_command=許可リストにないコマンドを許可に加えて実行。 */
export type ApprovalKind = "file" | "plan" | "command" | "allow_command";

/** 音声対話中のファイル操作・作業の進捗。`approval`は実行前の承認要求で、`agentApprove`で応答する。 */
export type AgentEvent =
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
  | { type: "command_output"; id: string; stream: "stdout" | "stderr"; text: string };

export const agentApprove = (id: string, approved: boolean) =>
  invoke<void>("agent_approve", { id, approved });
/** 実行中の依頼を中断する。動いているコマンドは強制終了され、承認待ちは拒否になる。 */
export const agentCancel = () => invoke<void>("agent_cancel");

/** シェルコマンドによる作業の可否(Claude選択時のみ有効)。未設定なら有効。 */
export const getShellAccess = () => invoke<boolean>("get_shell_access");
export const setShellAccess = (enabled: boolean) => invoke<void>("set_shell_access", { enabled });

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

/** 設定ページで変更できる数値の調整項目。保存時に範囲内へ丸められる。 */
export type Tuning = {
  web_search_max: number;
  web_fetch_max: number;
  voice_idle_secs: number;
  utterance_silence_secs: number;
  release_tail_secs: number;
  history_exchanges: number;
};
export const DEFAULT_TUNING: Tuning = {
  web_search_max: 5,
  web_fetch_max: 5,
  voice_idle_secs: 3,
  utterance_silence_secs: 1.2,
  release_tail_secs: 0.5,
  history_exchanges: 10,
};
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

export type SttEngine = "local" | "google" | "aqua";
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

export const ttsKeyRegistered = () => invoke<boolean>("tts_key_registered");
export const saveTtsKey = (key: string) => invoke<void>("save_tts_key", { key });
export const deleteTtsKey = () => invoke<void>("delete_tts_key");

export const aquaKeyRegistered = () => invoke<boolean>("aqua_key_registered");
export const saveAquaKey = (key: string) => invoke<void>("save_aqua_key", { key });
export const deleteAquaKey = () => invoke<void>("delete_aqua_key");

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
let speaking = false;
/** 再生中の読み上げの情報。字幕を再生位置に合わせるために使う。`text`は読み上げている本文。 */
let playback: { text: string; startedAt: number; duration: number } | null = null;

/** 読み上げ音声を再生中か。 */
export const isSpeaking = () => speaking;

/** `text`を読み上げ中なら、再生の進行度(0〜1)。それ以外はnull。 */
export function speechProgress(text: string): number | null {
  if (!speaking || !playback || !audioCtx || playback.text !== text) return null;
  return Math.min(1, Math.max(0, (audioCtx.currentTime - playback.startedAt) / playback.duration));
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
    audioCtx ??= new AudioContext();
    if (audioCtx.state === "suspended") void audioCtx.resume();
  } catch {
    // 非対応環境ではHTMLAudioへフォールバックする
  }
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
  clearSpeakingTimer();
  speaking = false;
  playback = null;
}

/**
 * 再生を開始する。直前の再生は止める。再生できない場合は`playback_error`で失敗する。
 * `text`は読み上げる本文。渡すと`speechProgress`で再生位置を取れる(定型音声など本文がないものは省く)。
 */
export async function playBase64Mp3(b64: string, text?: string): Promise<void> {
  stopSpeech();
  try {
    // 操作なしで再生を始めると(常時待機の応答など)中断状態のままになり、終了通知が来ず再生中扱いが残るため、再生できなければ失敗にする
    const ctx = await runningAudioContext();
    const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
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
};

/** ナレッジ(Markdown)の保存先フォルダ。Obsidianのvaultとして開ける。 */
export const knowledgeDir = () => invoke<string>("knowledge_dir");
/** 保存先を変更する(nullで既定に戻す)。既存ファイルは移動されない。変更後のパスを返す。 */
export const setKnowledgeDir = (path: string | null) => invoke<string>("set_knowledge_dir", { path });
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
export const getBatchInterval = () => invoke<number>("get_batch_interval");
export const setBatchInterval = (minutes: number) => invoke<void>("set_batch_interval", { minutes });
export const runBatch = () => invoke<BatchReport | null>("run_batch");

export type SettingsTransferReport = { applied: number; skipped: number };
/** 設定をJSONファイルへ書き出す。APIキーは含まれない。 */
export const exportSettings = (dest: string) => invoke<SettingsTransferReport>("export_settings", { dest });
/** 書き出した設定ファイルを読み込んで反映する。 */
export const importSettings = (src: string) => invoke<SettingsTransferReport>("import_settings", { src });

/** トレイメニューの表示言語を切り替える。 */
export const setTrayLanguage = (lang: string) => invoke<void>("set_tray_language", { lang });
