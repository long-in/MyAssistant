"use client";

import { useT } from "@/lib/i18n";
import { useCallback, useEffect, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { toast } from "sonner";
import { approvalLine } from "@/components/AttentionToasts";
import { notifyOsIfBackground } from "@/lib/osNotify";
import { VoiceOrb } from "./Waveforms";
import ChatThread from "./ChatThread";
import ChatComposer from "./ChatComposer";
import ChatNotices, { type SetupState } from "./ChatNotices";
import ApprovalModal, { type Approval, type Listening } from "./ApprovalModal";
import SwitchConfirm from "./SwitchConfirm";
import DispatchDraftCard from "./DispatchDraftCard";
import LeaderLedger from "./LeaderLedger";
import InfoCard from "./InfoCard";
import { applyTaskEvent, emptyLog } from "./ExecutionLog";
import { type Turn } from "./Reply";
import OrbReadout, { type Phase } from "./OrbReadout";
import { useDownloadProgress } from "./DownloadBar";
import { cn } from "@/lib/utils";
import { isPlainSpace, matchShortcut } from "@/lib/shortcuts";
import { judgeAnswer } from "@/lib/voiceApproval";
import { detectSwitch } from "@/lib/voiceCommands";
import {
  LEADER_ID,
  type TaskEvent,
  type DispatchDraft,
  type KeyStatus,
  type ModelInfo,
  type Route,
  type RunSnapshot,
  type SessionTurn,
  chatSessionLoad,
  chatSessionNew,
  DEFAULT_VOICE_TIMING,
  type VoiceTiming,
  type WakeConfig,
  taskApprove,
  taskDetach,
  taskResume,
  taskCancel,
  cancelRecording,
  downloadSttModel,
  sttDownloadProgress,
  getSttEngine,
  getTtsEngine,
  ttsKeyRegistered,
  getKeyStatus,
  getVoiceTiming,
  getWakeConfig,
  isSpeaking,
  listModels,
  loadRouting,
  playBase64Mp3,
  recordingWaveform,
  getSandboxEnabled,
  getSandboxStatus,
  getSandboxModels,
  setSandboxModel,
  type SandboxModels,
  startAgentSandbox,
  stopAgentSandbox,
  resetSandbox,
  type SandboxStatus,
  reportError,
  saveRouting,
  sendChat,
  setWakeConfig,
  startRecording,
  stopSpeech,
  unlockAudio,
  stopRecordingAndCheckWake,
  stopRecordingAndTranscribe,
  sttModelReady,
  synthesizeSpeech,
  trimRecording,
  ttsReady as fetchTtsReady,
  voicePhraseAudio,
  waitForSpeechEnd,
} from "@/lib/api";

/** サンドボックスで、このアプリにモデルが無いときに送る仮の経路(バックエンドでは使わない)。 */
const SANDBOX_ROUTE: Route = { provider: "anthropic", model: "sandbox" };

// 録音の待ち時間(エージェントを離したあとの猶予・声が入らず終える時間・話し終わりの無音)は、全体設定で変えられる(`voice_timing`)。

/** 話し始めを待つ時間(ms)。超えたら録り直す。 */
const ANSWER_WAIT_MS = 12000;
/** 発話とみなす音量(0〜1)の下限。周囲のノイズ(観測した最小値)の数倍と比べて大きい方を使う。 */
const SPEECH_PEAK_MIN = 0.03;
/** 1回の発話として扱う長さの上限(ms)。話し続けても、ここで区切って文字起こしする。 */
const MAX_UTTERANCE_MS = 30000;

/** 常時待機: 話し出すまでに残しておく直前の入力の長さ(ms)。出だしが欠けないようにする。 */
const STANDBY_PRE_ROLL_MS = 1000;
/** 常時待機: 周囲の音量の基準(最小値)を取り直す間隔(ms)。環境音の変化に追従する。 */
const STANDBY_FLOOR_RESET_MS = 10000;
/** 常時待機: 読み上げや処理が終わってから、待機の録音を再開するまでの間(ms)。残響を拾わないため。 */
const STANDBY_RESUME_MS = 600;

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

/** ナレッジのファイル操作・コマンド実行中に表示する文言。 */
const TOOL_ACTIVITY: Record<string, string> = {
  propose_plan: "手順を考えています…",
  run_command: "コマンドを実行しています…",
  list_files: "ファイルを一覧しています…",
  read_file: "ファイルを読んでいます…",
  search_files: "ファイルを検索しています…",
  create_file: "ファイルを作成しています…",
  agent_list: "エージェントの一覧を確認しています…",
  ledger_list: "台帳を確認しています…",
  draft_dispatch: "配布の下書きを作っています…",
  remember: "覚えています…",
  forget: "記憶から外しています…",
  use_skill: "スキルの手順を読んでいます…",
  create_skill: "スキルを作成しています…",
  update_skill: "スキルを更新しています…",
  delete_skill: "スキルを削除しています…",
  // サンドボックス(コンテナ内の Claude Code)のツール
  sandbox_prepare: "サンドボックスを準備しています…",
  Bash: "コマンドを実行しています…",
  Read: "ファイルを読んでいます…",
  Write: "ファイルを作成しています…",
  Edit: "ファイルを編集しています…",
  NotebookEdit: "ファイルを編集しています…",
  Glob: "ファイルを検索しています…",
  Grep: "ファイルを検索しています…",
  WebFetch: "ページを読んでいます…",
  WebSearch: "Webを検索しています…",
  Task: "サブエージェントに任せています…",
  TodoWrite: "作業を整理しています…",
};

/** `standby`: 常時待機が起動ワードを聞き取るために録音している。手動の操作が始まれば譲る。 */
type Mic = "idle" | "starting" | "recording" | "transcribing" | "standby";

/** 録音中の入力を見て話し終えるのを待った結果。`lost`は手動操作などで録音の持ち主が変わったこと。 */
type Capture = "spoke" | "silent" | "lost";
/** 録音して文字起こしした結果。`retry`は聞き取れなかった、`stop`はマイクが使えない。 */
type Heard = { text: string } | "retry" | "stop";

/** `active`は対話画面を表示中かどうか。他の画面へ移動しても`Chat`は`AppShell`に残り、進行中の処理や待機を続ける。 */
export default function Chat({ workspaceId, active }: { workspaceId: string; active: boolean }) {
  const t = useT();
  const activeRef = useRef(active);
  useEffect(() => {
    activeRef.current = active;
  }, [active]);
  // 会話の切り替えは、ユーザーが明示したときだけ。コマンド実行の依頼・承認や画面移動では切り替えない(再起動後は、バックエンドが前回までの文脈をMarkdownから復元する)
  // 現在の会話は、エージェントごとにDBへ保存してあり、アプリを再起動しても続きから再開する(読み込むまでIDは空)
  const session = useRef({ id: "" });
  const [sessionReady] = useState(() => {
    let resolve: () => void = () => {};
    const promise = new Promise<void>((r) => (resolve = r));
    return { promise, resolve };
  });
  // この会話(セッション)のやり取り。最後が現在の1件で、会話を切り替えるまで残す
  const [turns, setTurns] = useState<Turn[]>([]);
  const [busy, setBusy] = useState(false);
  const [draft, setDraft] = useState("");
  const [activity, setActivity] = useState<string | null>(null);
  const [approval, setApproval] = useState<Approval | null>(null);
  // リーダーが作った配布の下書き。利用者が確かめて配布する
  const [dispatchDraft, setDispatchDraft] = useState<DispatchDraft | null>(null);
  const router = useRouter();

  // 承認のダイアログは対話画面にある。設定などの別の画面を見ている間に承認が必要になったら、トーストで知らせる(「開く」で対話画面へ)。
  // 承認に答えた・対話画面へ戻ったら、トーストを消す。ほかのエージェントの通知(AttentionToasts)と、同じ識別子で重ならないようにする
  useEffect(() => {
    const id = `attention:${workspaceId}:approval`;
    if (!approval || active) {
      toast.dismiss(id);
      return;
    }
    const one = (text: string) => {
      const line = text.split(/\s+/).filter(Boolean).join(" ");
      return line.length > 120 ? `${line.slice(0, 120)}…` : line;
    };
    const detail = approval.kind === "file" ? approval.path : approval.kind === "command" ? approval.preview : approval.kind === "allow_command" ? approval.steps.join(", ") : "";
    const title = t("承認が必要です");
    const description = approvalLine(approval.kind, one(detail));
    toast.warning(title, { id, description, duration: Infinity, action: { label: t("開く"), onClick: () => router.push("/") } });
    void notifyOsIfBackground(title, description);
  }, [approval, active, workspaceId, router, t]);
  // 音声の返答の待ち受けは非同期で動くので、最新の承認要求をrefでも持つ
  const approvalRef = useRef<Approval | null>(null);
  const [listening, setListening] = useState<Listening>("idle");
  const [route, setRoute] = useState<Route | null>(null);
  /** サンドボックスが有効なら、会話はコンテナ内の Claude Code が受け持つ(このアプリのAPIキーやモデルの選択は使わない)。 */
  const [sandboxOn, setSandboxOn] = useState(false);
  useEffect(() => {
    getSandboxEnabled().then(setSandboxOn).catch(() => {});
  }, []);
  /** サンドボックスのコンテナが使える状態か(未確認は null)。使えるまでは、指示を送れない。 */
  const [sandboxReady, setSandboxReady] = useState<boolean | null>(null);
  const [sandboxStatus, setSandboxStatus] = useState<SandboxStatus | null>(null);
  /** チャット画面からのコンテナの起動・停止の実行中。 */
  const [containerBusy, setContainerBusy] = useState(false);
  const refreshSandbox = useCallback(
    () =>
      getSandboxStatus()
        .then((s) => {
          setSandboxStatus(s);
          setSandboxReady(s.docker === "ready" && s.image_ready && s.container_ready);
        })
        .catch(() => {}),
    [],
  );
  useEffect(() => {
    // 状態はアクティブなエージェントのものだけが取れる
    if (!sandboxOn || !active) return;
    let alive = true;
    const check = () => (alive ? refreshSandbox() : Promise.resolve());
    void check();
    const timer = setInterval(() => void check(), sandboxReady ? 30_000 : 5_000);
    window.addEventListener("focus", check);
    return () => {
      alive = false;
      clearInterval(timer);
      window.removeEventListener("focus", check);
    };
  }, [sandboxOn, active, sandboxReady, refreshSandbox]);
  const needSandbox = sandboxOn && sandboxReady === false;
  /** サンドボックスで選べるモデル(settings.json の許可)。設定画面で書き換えたあと戻ってきたときに読み直す。 */
  const [sandboxModels, setSandboxModels] = useState<SandboxModels | null>(null);
  useEffect(() => {
    if (!sandboxOn || !active) return;
    let alive = true;
    const load = () =>
      getSandboxModels()
        .then((m) => alive && setSandboxModels(m))
        .catch(() => {});
    void load();
    window.addEventListener("focus", load);
    return () => {
      alive = false;
      window.removeEventListener("focus", load);
    };
  }, [sandboxOn, active]);
  const changeSandboxModel = (model: string) => {
    setSandboxModels((m) => (m ? { ...m, selected: model } : m));
    setSandboxModel(model).catch(fail);
  };
  /** コンテナを起動する。起動中なら停止する。 */
  const toggleContainer = async () => {
    if (containerBusy) return;
    setContainerBusy(true);
    try {
      if (sandboxReady) {
        await stopAgentSandbox();
        toast.success(t("コンテナを停止しました。"));
      } else {
        if (sandboxStatus?.container_name && !sandboxStatus.container_ready) await resetSandbox(); // 古いコンテナは作り直す
        await startAgentSandbox();
        toast.success(t("コンテナを起動しました。"));
      }
      await refreshSandbox();
    } catch (e) {
      fail(e);
    } finally {
      setContainerBusy(false);
    }
  };
  const containerStartable = !!sandboxStatus && sandboxStatus.docker === "ready" && sandboxStatus.image_ready && !sandboxStatus.building;
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [keyStatus, setKeyStatus] = useState<Partial<KeyStatus>>({});
  const [mic, setMicState] = useState<Mic>("idle");
  // キーの離上はレンダリングを待たず最新の状態で判定したいため、refにも持つ
  const micNow = useRef<Mic>("idle");
  const setMic = (m: Mic) => {
    micNow.current = m;
    setMicState(m);
  };
  // 非同期の途中で変わりうる状態として読む(型が直前の比較で絞られないように関数にする)
  const micState = (): Mic => micNow.current;
  const keyHeld = useRef(false);
  const [needModel, setNeedModel] = useState(false);
  const [downloading, setDownloading] = useState(false);
  // 別の画面で始めたモデルの取得が続いていれば、戻ったときも進み具合を見せる
  const { progress: dlProgress, active: dlActive } = useDownloadProgress(sttDownloadProgress, downloading, () => setNeedModel(false));
  const [ttsReady, setTtsReady] = useState(false);
  /** 設定不足の案内に使う、読み込み済みの設定の状況。読み込むまでは案内しない。 */
  const [setup, setSetup] = useState<SetupState | null>(null);
  const [voiceReply, setVoiceReply] = useState(true);
  /** Voice Linkのキーが未登録なら読み上げず、字幕だけで応答する。 */
  const speakReply = voiceReply && ttsReady;
  /** 音声の合成を待つ間にミュートされたときに、再生を取りやめるための最新値。 */
  const speakReplyRef = useRef(speakReply);
  useEffect(() => {
    speakReplyRef.current = speakReply;
  }, [speakReply]);
  const [speaking, setSpeaking] = useState(false);
  const [wake, setWake] = useState<WakeConfig>({ enabled: false, words: [] });
  /** 起動ワードを受けて、用件を待っている間。 */
  const [awake, setAwake] = useState(false);
  const ackAudio = useRef<string | null>(null);
  // 常時待機は長く動き続けるので、描画ごとに変わる値は参照経由で最新を読む
  const gate = useRef({ busy: false, needModel: false });
  const wakeHandler = useRef<(text: string, alive: () => boolean) => Promise<void>>(async () => {});
  const standbyRun = useRef<(alive: () => boolean) => Promise<void>>(async () => {});
  const holdRef = useRef<(down: boolean) => void>(() => {});
  const micLock = useRef(false);
  const tuning = useRef<VoiceTiming>(DEFAULT_VOICE_TIMING);
  const mounted = useRef(true);
  /** 手動の録音ごとの通し番号。無音の見張りが、古い録音に反応しないようにする。 */
  const manualRun = useRef(0);
  /** 返答のあとの会話の続きを聞いている間。 */
  const conversing = useRef(false);
  /** 「新しい会話」の操作ごとに増える番号。会話中のループは、開始時と違えば終わる。 */
  const resetEpoch = useRef(0);
  /** 会話の切り替えの確認中。答えが出るまで解決を待つ関数を持つ。 */
  const [switching, setSwitching] = useState(false);
  const switchResolve = useRef<((yes: boolean) => void) | null>(null);
  const [switchListening, setSwitchListening] = useState<Listening>("idle");
  const standbyOn = wake.enabled && wake.words.length > 0;
  /** エージェントを離してから録音を止めるまでの待ちタイマー。語尾が切れないよう少し録り続ける。 */
  const tailTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const toggleRef = useRef<() => Promise<void>>(async () => {});
  const clearTail = () => {
    if (tailTimer.current) clearTimeout(tailTimer.current);
    tailTimer.current = null;
  };
  const stopAfterTail = () => {
    clearTail();
    tailTimer.current = setTimeout(() => {
      tailTimer.current = null;
      if (!keyHeld.current && micNow.current === "recording") void toggleRef.current();
    }, tuning.current.release_tail_secs * 1000);
  };

  const fail = (e: unknown) => toast.error(reportError(e));

  // 対話画面へ戻るたびに、設定画面などでの変更(キー・モデル・待機設定)を読み直す
  useEffect(() => {
    if (!active) return;
    Promise.all([listModels(), loadRouting(), getKeyStatus(), fetchTtsReady(), getWakeConfig(), getVoiceTiming(), getTtsEngine(), getSttEngine(), ttsKeyRegistered(), sttModelReady()])
      .then(([m, r, k, t, w, u, te, se, gk, sm]) => {
        tuning.current = u;
        setSetup({ tts: te, stt: se, sttKey: se === "google" ? gk : true });
        setNeedModel(se === "local" && !sm);
        setModels(m);
        setRoute(r);
        setKeyStatus(k);
        setTtsReady(t);
        setWake(w);
      })
      .catch(fail);
  }, [active]);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      // 待っている送信・会話のループを打ち切る(離れたあとに返答が届いても、読み上げたり、やり取りへ加えたりしない。
      // 依頼そのものは止まらず、結果は戻ったときに受け取る)
      // eslint-disable-next-line react-hooks/exhaustive-deps -- DOMではなくカウンタ。離れる時点の最新の値を進める
      resetEpoch.current++;
      // ワークスペースの切り替えで作り直されるとき、読み上げと録音を止める(切替先へ音声を持ち越さない)
      stopSpeech();
      void cancelRecording().catch(() => {});
    };
  }, []);

  useEffect(() => {
    const id = setInterval(() => setSpeaking(isSpeaking()), 150);
    return () => clearInterval(id);
  }, []);

  const changeRoute = (r: Route) => {
    setRoute(r);
    saveRouting(r).catch(fail);
  };

  const showApproval = (a: Approval | null) => {
    approvalRef.current = a;
    setApproval(a);
  };

  // 記憶のツール呼び出し(id → 種類と文面)。結果が成功したら、覚えた・忘れたことを知らせる
  const memoryCalls = useRef(new Map<string, { name: string; text: string }>());
  const onTaskEvent = (e: TaskEvent) => {
    // 実行ログは、いま処理している発話(最後)に結び付ける
    setTurns((ts) => ts.map((x, i) => (i === ts.length - 1 ? { ...x, log: applyTaskEvent(x.log ?? emptyLog, e) } : x)));
    if (e.type === "tool_call") {
      setActivity(t(TOOL_ACTIVITY[e.name] ?? "ファイルを操作しています…"));
      if ((e.name === "remember" || e.name === "forget") && typeof e.input.text === "string")
        memoryCalls.current.set(e.id, { name: e.name, text: e.input.text });
    } else if (e.type === "tool_result") {
      setActivity(null);
      const call = memoryCalls.current.get(e.id);
      memoryCalls.current.delete(e.id);
      if (call && e.ok) toast.success(t(call.name === "remember" ? "覚えました: {text}" : "忘れました: {text}", { text: call.text }));
    }
    else if (e.type === "approval") showApproval(e);
    else if (e.type === "dispatch_draft") setDispatchDraft({ id: e.id, instruction: e.instruction, targets: e.targets });
    else if (e.type === "finished") finishResumed(e);
  };

  /** 切り替えから戻って続きを受けていた依頼が終わった。結果をやり取りに加える(読み上げはしない)。 */
  const finishResumed = (e: Extract<TaskEvent, { type: "finished" }>) => {
    setBusy(false);
    setActivity(null);
    showApproval(null);
    const { reply, error } = e;
    if (error) fail(error);
    if (reply) {
      setTurns((ts) =>
        ts.map((x, i) => (i === ts.length - 1 && !x.reply ? { ...x, reply: reply.content, replyAt: new Date(), via: reply.model } : x)),
      );
    }
  };

  /** 切り替えて離れている間の依頼を、続きから再開する(実行中なら進捗と承認待ちを戻し、終わっていれば結果を見せる)。 */
  const restoreRun = (snap: RunSnapshot) => {
    if (!snap.running && !snap.finished) return;
    const log = snap.events.reduce(applyTaskEvent, emptyLog);
    // 発話はDBから戻した会話にもある。返答の無い最後の同じ発話なら、それに実行ログや返答を加える
    const merge = (ts: Turn[], patch: Partial<Turn>): Turn[] => {
      const i = ts.length - 1;
      if (i >= 0 && ts[i].user === snap.user_text && !ts[i].reply) return ts.map((x, j) => (j === i ? { ...x, ...patch } : x));
      if (i >= 0 && patch.reply && ts[i].reply === patch.reply) return ts; // 終わった結果は、すでに会話に戻っている
      return [...ts, { user: snap.user_text, userAt: new Date(), ...patch }];
    };
    if (snap.finished) {
      // 離れている間に終わっていた。結果だけを見せる
      const { reply, error } = snap.finished;
      if (error) fail(error);
      if (reply) setTurns((ts) => merge(ts, { log, reply: reply.content, replyAt: new Date(), via: reply.model }));
      return;
    }
    setBusy(true);
    setTurns((ts) => merge(ts, { log }));
    const lastDraft = [...snap.events].reverse().find((x) => x.type === "dispatch_draft");
    if (lastDraft?.type === "dispatch_draft") setDispatchDraft({ id: lastDraft.id, instruction: lastDraft.instruction, targets: lastDraft.targets });
    // 離れている間に求められた承認は、回答を待っている。戻ったので、ここで答えられる
    if (snap.pending) showApproval({ type: "approval", ...snap.pending });
  };
  const taskEventRef = useRef(onTaskEvent);
  const restoreRef = useRef(restoreRun);
  useEffect(() => {
    taskEventRef.current = onTaskEvent;
    restoreRef.current = restoreRun;
  });

  // このワークスペースを開いたとき、前回までの会話を戻し、実行中だった依頼の続きを受け取る。離れるときは、依頼を止めずに接続だけ外す
  useEffect(() => {
    let live = true;
    const toTurn = (x: SessionTurn): Turn => ({
      user: x.user,
      userAt: new Date(x.user_at),
      reply: x.reply ?? undefined,
      replyAt: x.reply_at ? new Date(x.reply_at) : undefined,
      via: x.via ?? undefined,
      log: x.events.reduce(applyTaskEvent, emptyLog),
      restored: true,
    });
    chatSessionLoad()
      .then((view) => {
        if (!live) return;
        session.current = { id: view.session_id };
        setTurns(view.turns.map(toTurn));
      })
      .catch((e) => {
        // 読み込めなくても、新しい会話として続けられるようにする
        if (!live) return;
        fail(e);
        session.current = { id: crypto.randomUUID() };
      })
      .then(() => {
        sessionReady.resolve();
        return live ? taskResume(workspaceId, (e) => live && taskEventRef.current(e)) : null;
      })
      .then((snap) => live && snap && restoreRef.current(snap))
      .catch(() => {});
    return () => {
      live = false;
      taskDetach(workspaceId).catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 読み込みはワークスペースごとに1回
  }, [workspaceId]);

  const answerApproval = (approved: boolean) => {
    const a = approvalRef.current;
    if (!a) return;
    showApproval(null);
    taskApprove(a.id, approved).catch(fail);
  };

  /** 承認待ちへの音声の返答。肯定とはっきり分かるときだけ進め、聞き取れなければ待ち続ける。 */
  const answerByVoice = (text: string) => {
    const answer = judgeAnswer(text);
    if (answer === "yes") answerApproval(true);
    else if (answer === "no") answerApproval(false);
    else toast.info(t("「はい」または「いいえ」でお答えください。"));
  };

  /** 実行中の依頼を中断する。動いているコマンドは止まり、承認待ちは拒否になる。 */
  const cancelTask = () => {
    showApproval(null);
    taskCancel().catch(fail);
  };

  /** 新しい会話を始める。これまでの文脈は引き継がず、実行中の処理と読み上げは止める。 */
  const startNewConversation = (interrupt: boolean) => {
    const next = { id: crypto.randomUUID() };
    session.current = next;
    chatSessionNew(next.id).catch(fail); // 再起動後も、この新しい会話から再開する
    if (interrupt) resetEpoch.current++; // 会話中のループは自分で続けるので、止めるのは外からの操作のときだけ
    stopSpeech();
    if (gate.current.busy || approvalRef.current) cancelTask();
    setTurns([]);
    setDispatchDraft(null);
    toast.success(t("新しい会話を始めました。"));
  };

  /**
   * 進行中の処理をすべて強制的に止める(雑音や話し声で意図せず進んだときの停止手段)。
   * 読み上げ・録音・文字起こしの結果・実行中の依頼・承認待ち・切り替えの確認を打ち切り、会話の続きの待ち受けも終える。
   * 会話の文脈は残す。待機がオンなら、そのまま起動ワードの待ち受けに戻る。
   */
  const abortAll = () => {
    resetEpoch.current++; // 会話中のループ・待ち受け・返答の読み上げは、番号の違いで終わる
    manualRun.current++;
    clearTail();
    stopSpeech();
    cancelTask();
    if (switchResolve.current) answerSwitch(false);
    if (micNow.current === "recording" || micNow.current === "standby") {
      void cancelRecording().catch(() => {});
      setMic("idle");
    }
    setListening("idle");
    setAwake(false);
    setActivity(null);
    toast.info(t("中断しました。"));
  };
  const abortRef = useRef(abortAll);
  const working = useRef(false);

  /** 依頼を送る。`spoke`: 返答を読み上げた、`replied`: 返答は画面のみ、`none`: 返答なし。 */
  const submit = async (raw: string): Promise<"spoke" | "replied" | "none"> => {
    const text = raw.trim();
    if (!text || busy) return "none";
    unlockAudio(); // 操作のタイミングで有効化し、応答の自動再生を可能にする
    await sessionReady.promise; // 前回の会話の読み込みを待つ
    const sessionId = session.current.id;
    const epoch = resetEpoch.current;
    const aborted = () => session.current.id !== sessionId || resetEpoch.current !== epoch;
    const userAt = new Date();
    setBusy(true);
    setTurns((ts) => [...ts, { user: text, userAt }]);
    let result: "spoke" | "replied" | "none" = "none";
    try {
      // サンドボックスでは、経路はバックエンドで使われない(接続先はエージェントの Sandbox の設定)
      const sendRoute = route ?? (sandboxOn ? SANDBOX_ROUTE : null);
      if (!sendRoute) throw new Error(t("使えるモデルがありません。設定でAPIキーを登録してください。"));
      const r = await sendChat(sessionId, text, sendRoute, null, onTaskEvent);
      if (aborted()) return "none"; // 待っている間に新しい会話へ切り替わった、または中断された
      const replied = { reply: r.content, replyAt: new Date(), via: r.model };
      result = "replied";
      if (speakReply) {
        // 字幕を読み上げの再生位置に合わせるため、音声を作って再生を始めてから返答を表示する
        const audio = await synthesizeSpeech(r.content).catch((e) => {
          fail(e);
          return null;
        });
        if (aborted()) return "none";
        if (audio && speakReplyRef.current) await playBase64Mp3(audio, r.content).catch(fail);
        result = "spoke";
      }
      setTurns((ts) => ts.map((x) => (x.userAt === userAt ? { ...x, ...replied } : x)));
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
      setActivity(null);
      showApproval(null);
    }
    return result;
  };

  /** 字幕の返答を、読み上げボタンから再生する。ミュート中でも、押したときは読み上げる。 */
  const speakOnDemand = async (text: string) => {
    unlockAudio();
    stopSpeech();
    try {
      const audio = await synthesizeSpeech(text);
      await playBase64Mp3(audio, text);
    } catch (e) {
      fail(e);
    }
  };

  /** 入力欄の文章を送る。送信が始まったら欄を空にし、読み上げ中なら止めてから送る。 */
  const submitDraft = () => {
    const text = draft.trim();
    if (!text || busy || needModel || needSandbox || mic !== "idle") return;
    stopSpeech();
    setDraft("");
    void submit(text);
  };

  /** `hold`: エージェント長押しでの開始。開始処理中にキーが離されていたら、録音できた時点でそのまま停止する。 */
  const toggleMic = async (hold = false) => {
    // 開始・停止処理の最中の再入力(キーのリピートや連打)は無視し、録音を勝手に止めない
    const state = micNow.current;
    if (micLock.current || state === "starting" || state === "transcribing") return;
    if (state !== "recording" && isSpeaking()) return; // 読み上げ中は手動でもマイクを開かない(音声を拾って割り込むため)
    micLock.current = true;
    unlockAudio();
    const epoch = resetEpoch.current;
    let text: string | null = null;
    try {
      const stop = state === "recording";
      if (!stop) {
        if ((await getSttEngine()) === "local" && !(await sttModelReady())) {
          setNeedModel(true);
          return;
        }
        setMic("starting"); // マイクが音を届け始めるまで約2秒かかる。録音中表示はその後
        if (state === "standby") await cancelRecording().catch(() => {}); // 常時待機の録音は手動操作に譲る
        await startRecording();
        setMic("recording");
        if (hold && !keyHeld.current) stopAfterTail(); // 開始待ちの間に離されていた
        void autoStopManual(hold);
      }
      if (stop) {
        clearTail();
        manualRun.current++; // 無音監視を終わらせる
        setMic("transcribing");
        text = await stopRecordingAndTranscribe(session.current.id, !!approvalRef.current);
        setMic("idle");
      }
    } catch (e) {
      // すでにバックエンドが録音中なら、UIを録音中に合わせる
      setMic((e as { code?: string })?.code === "already_recording" ? "recording" : "idle");
      fail(e);
    } finally {
      micLock.current = false;
    }
    if (text && resetEpoch.current === epoch) {
      // 承認待ちの間の発話は、新しい依頼ではなく承認への返答として扱う
      if (approvalRef.current) answerByVoice(text);
      else await converse(text, () => mounted.current);
    }
  };

  /** 手動の録音を見張り、設定した秒数、声が入らなければ自動で終える。話していれば送信し、無言なら破棄する。 */
  const autoStopManual = async (hold: boolean) => {
    const run = ++manualRun.current;
    const mine = () => mounted.current && manualRun.current === run;
    const idleMs = tuning.current.voice_idle_secs * 1000;
    const heard = await captureSpeech(mine, "recording", idleMs, 0, idleMs, hold ? () => keyHeld.current : undefined);
    if (!mine() || micState() !== "recording") return;
    if (heard === "spoke") {
      await toggleMic();
    } else if (heard === "silent") {
      clearTail();
      await cancelRecording().catch(() => {});
      setMic("idle");
    }
  };

  /** 返答を読み上げ終えたあとに声が入るのを待ち、続きの依頼の文字を返す。なければ空。 */
  const listenFollowUp = async (alive: () => boolean): Promise<string> => {
    setAwake(true);
    try {
      const heard = await listenOnce(alive, tuning.current.voice_idle_secs * 1000);
      return heard === "retry" || heard === "stop" ? "" : heard.text;
    } finally {
      setAwake(false);
    }
  };

  /** 切り替えの答えを画面のボタンまたは音声で受け、確認を閉じる。 */
  const answerSwitch = (yes: boolean) => {
    switchResolve.current?.(yes);
    switchResolve.current = null;
    setSwitching(false);
  };

  /**
   * 会話を切り替えてよいかを確認する。保存済みの確認音声を再生し(未作成・ミュート中は画面のみ)、
   * 「はい」「いいえ」の音声またはボタンの答えを待つ。聞き取れなければ聞き直し、答えがなければ「いいえ」。
   */
  const confirmSwitch = async (alive: () => boolean): Promise<boolean> => {
    const answer = new Promise<boolean>((resolve) => {
      switchResolve.current = resolve;
    });
    setSwitching(true);
    setSwitchListening("idle");
    const open = () => switchResolve.current !== null && alive();
    void (async () => {
      try {
        if (speakReply) {
          const audio = await voicePhraseAudio("switch_confirm").catch(() => null);
          if (audio) {
            await playBase64Mp3(audio);
            await waitForSpeechEnd();
            await sleep(STANDBY_RESUME_MS); // 残響を拾わない
          }
        }
        const deadline = Date.now() + ANSWER_WAIT_MS;
        while (open() && Date.now() < deadline) {
          setSwitchListening("listening");
          const heard = await listenOnce(open, deadline - Date.now());
          if (heard === "stop") break;
          if (heard !== "retry" && heard.text && open()) {
            const verdict = judgeAnswer(heard.text);
            if (verdict !== "unclear") return answerSwitch(verdict === "yes");
            toast.info(t("「はい」または「いいえ」でお答えください。"));
          }
        }
      } catch (e) {
        fail(e);
      } finally {
        setSwitchListening("idle");
      }
      if (switchResolve.current) answerSwitch(false); // 答えがなければ今の会話を続ける
    })();
    const yes = await answer;
    setSwitching(false);
    return yes;
  };

  /**
   * 依頼を送り、返答を読み上げたあとは自動で音声入力を再開して会話を続ける。
   * 常時待機が有効なら、読み上げがなくても続ける(起動ワードを呼び直さなくてよい)。
   * 設定した秒数、声が入らなければ、そこで会話を終える。
   */
  const converse = async (first: string, outer: () => boolean) => {
    conversing.current = true; // 常時待機の録音が割り込まないようにする
    const epoch = resetEpoch.current;
    const alive = () => outer() && resetEpoch.current === epoch; // 「新しい会話」の操作で終わる
    try {
      let text = first;
      while (text && alive()) {
        const intent = gate.current.busy ? null : detectSwitch(text);
        if (intent) {
          // 切り替えの表現は依頼として送らず、切り替えてよいか確認する
          if (await confirmSwitch(alive)) startNewConversation(false);
          if (!alive()) break;
          if (intent.kind === "command") {
            text = await listenFollowUp(alive);
            continue;
          }
        }
        const result = await submit(text);
        if (result === "none" || (result === "replied" && !standbyOn)) break;
        await waitForSpeechEnd();
        await sleep(STANDBY_RESUME_MS); // 残響を拾わない
        if (!alive()) break;
        text = await listenFollowUp(alive);
      }
    } finally {
      conversing.current = false;
    }
  };

  /**
   * 録音中の入力を見て、話し終えるまで待つ。
   * 一瞬の物音は発話にせず、話し出してから`endSilenceMs`(既定は設定の話し終わりの無音)無音が続いたら"spoke"。
   * `waitMs`(nullなら無期限)のあいだ話し出さなければ、`alive()`が偽になれば"silent"。録音の持ち主が`expect`でなくなれば"lost"。
   * `preRollMs`を指定すると、話し出すまでは直前のその長さの入力だけを残す(常時待機用)。
   * `endSilenceMs`は話し終わりとみなす無音の長さ。`held`が真の間(キー長押し中)は無音の計時を止める。
   */
  const captureSpeech = async (
    alive: () => boolean,
    expect: Mic,
    waitMs: number | null,
    preRollMs = 0,
    endSilenceMs = tuning.current.utterance_silence_secs * 1000,
    held?: () => boolean,
  ): Promise<Capture> => {
    let startedAt = Date.now();
    let floor = Infinity;
    let floorAt = startedAt;
    let loud = 0;
    let lastLoud = 0;
    let spokeAt = 0;
    while (alive() && micState() === expect) {
      await sleep(100);
      let peak = 0;
      try {
        peak = (await recordingWaveform()).reduce((m, x) => Math.max(m, Math.abs(x)), 0);
      } catch {
        return "silent";
      }
      const now = Date.now();
      if (held?.()) {
        startedAt = now;
        lastLoud = now;
      }
      if (preRollMs && !spokeAt && now - floorAt > STANDBY_FLOOR_RESET_MS) {
        floor = Infinity;
        floorAt = now;
      }
      floor = Math.min(floor, peak);
      if (peak > Math.max(SPEECH_PEAK_MIN, floor * 3 + 0.01)) {
        lastLoud = now;
        if (++loud >= 2 && !spokeAt) spokeAt = now; // 一瞬の物音は発話にしない
      } else {
        loud = 0;
      }
      if (spokeAt && (now - lastLoud > endSilenceMs || now - spokeAt > MAX_UTTERANCE_MS)) return "spoke";
      if (!spokeAt && waitMs !== null && now - startedAt > waitMs) return "silent";
      if (preRollMs && !spokeAt && loud === 0) void trimRecording(preRollMs).catch(() => {});
    }
    if (micState() !== expect) return "lost";
    return "silent";
  };

  /** 録音して話し終えるのを待ち、文字起こしして返す。 */
  const listenOnce = async (alive: () => boolean, waitMs: number): Promise<Heard> => {
    // 手動の録音操作の最中は、それが終わるまで待つ(返答はそちらの経路で判定される)
    if (micState() !== "idle" || micLock.current) {
      await sleep(300);
      return "retry";
    }
    // 読み上げ中は自分の声を拾うので、終わって残響が消えるまでマイクを開かない
    if (isSpeaking()) {
      await waitForSpeechEnd();
      await sleep(STANDBY_RESUME_MS);
      if (!alive()) return "stop";
    }
    setListening("preparing");
    setMic("starting");
    try {
      await startRecording();
    } catch (e) {
      setMic((e as { code?: string })?.code === "already_recording" ? "recording" : "idle");
      setListening("idle");
      fail(e);
      return "stop";
    }
    setMic("recording");
    setListening("listening");
    const heard = await captureSpeech(alive, "recording", waitMs);
    // 手動操作に引き継がれた場合は、そちらが録音を止める
    if (heard === "lost") return "retry";
    if (heard === "silent") {
      await cancelRecording().catch(() => {});
      setMic("idle");
      setListening("idle");
      return "retry";
    }
    setMic("transcribing");
    setListening("transcribing");
    try {
      const text = await stopRecordingAndTranscribe(session.current.id, !!approvalRef.current);
      setMic("idle");
      setListening("idle");
      return { text };
    } catch (e) {
      setMic("idle");
      setListening("idle");
      // 無音や短すぎる録音は聞き直す。マイクが使えないときは繰り返さない
      if ((e as { code?: string })?.code === "mic_no_signal") {
        fail(e);
        return "stop";
      }
      return "retry";
    }
  };

  /** 承認待ちの間、自動で録音して返答を聞く。話し終えたら文字起こしして判定し、聞き取れなければ録り直す。 */
  const listenForAnswer = async (alive: () => boolean) => {
    while (alive()) {
      const heard = await listenOnce(alive, ANSWER_WAIT_MS);
      if (heard === "stop") break;
      if (heard !== "retry" && alive() && heard.text) answerByVoice(heard.text);
    }
    setListening("idle");
  };

  /** 起動ワードを聞き取ったあと。続けて依頼があればそのまま送り、なければ応答して用件を待つ。 */
  const handleWake = async (rest: string, standbyAlive: () => boolean) => {
    const epoch = resetEpoch.current;
    const alive = () => standbyAlive() && resetEpoch.current === epoch; // 中断で終わる
    if (rest) {
      await converse(rest, alive);
      return;
    }
    try {
      // 作成済みの定型音声を再生する(未作成なら無音で用件を待つ。TTSへは送らない)
      ackAudio.current ??= await voicePhraseAudio("wake_ack");
      if (ackAudio.current && speakReply) {
        await playBase64Mp3(ackAudio.current);
        await waitForSpeechEnd();
      }
    } catch (e) {
      fail(e);
    }
    const text = await listenFollowUp(alive);
    if (text && alive()) await converse(text, alive);
    else if (!text && micState() === "idle") toast.info(t("用件が聞き取れませんでした。もう一度呼びかけてください。"));
  };

  /**
   * 常時待機。マイクを開いたまま、話し出して話し終えるたびに文字起こしし、起動ワードを含むかだけを調べる。
   * 含まない発話は校正も送信もされない(判定はバックエンドで、LLMへ回す前に行う)。
   */
  const standbyLoop = async (alive: () => boolean) => {
    let quietSince = Date.now();
    await sleep(STANDBY_RESUME_MS); // 画面を開いた直後はマイクを開かず、他の初期化を先に済ませる
    const blocked = () => gate.current.busy || gate.current.needModel || !!approvalRef.current || conversing.current || isSpeaking();
    while (alive()) {
      if (blocked()) quietSince = Date.now(); // 読み上げや処理の音を拾わない
      if (blocked() || micState() !== "idle" || micLock.current || Date.now() - quietSince < STANDBY_RESUME_MS) {
        await sleep(200);
        continue;
      }
      micLock.current = true;
      try {
        setMic("starting");
        await startRecording();
      } catch (e) {
        setMic((e as { code?: string })?.code === "already_recording" ? "recording" : "idle");
        fail(e);
        return;
      } finally {
        micLock.current = false;
      }
      setMic("standby");
      const heard = await captureSpeech(() => alive() && !blocked(), "standby", null, STANDBY_PRE_ROLL_MS);
      if (heard === "lost") continue; // 手動の録音に譲った
      if (heard === "silent") {
        await cancelRecording().catch(() => {});
        setMic("idle");
        continue;
      }
      setMic("transcribing");
      try {
        const check = await stopRecordingAndCheckWake(session.current.id);
        setMic("idle");
        if (check.woke && alive()) await wakeHandler.current(check.text, alive);
      } catch (e) {
        setMic("idle");
        const code = (e as { code?: string })?.code;
        // 短い・無音の録音は待機中によくあるので黙って聞き直す。マイクが使えないときは繰り返さない
        if (code === "mic_no_signal") {
          fail(e);
          return;
        }
        if (code && !["too_short", "no_audio", "mic_too_quiet"].includes(code)) fail(e);
      }
    }
  };

  // 待機ループの開始より先に、最新の関数を参照へ入れておく(効果は宣言順に実行される)
  useEffect(() => {
    wakeHandler.current = handleWake;
    standbyRun.current = standbyLoop;
  });
  useEffect(() => {
    if (!standbyOn) return;
    let live = true;
    void standbyRun.current(() => live);
    return () => {
      live = false;
    };
  }, [standbyOn]);

  // 起動ワードへの応答音声は先に読み込んでおき、呼びかけてすぐ返せるようにする
  useEffect(() => {
    if (standbyOn && !ackAudio.current) {
      voicePhraseAudio("wake_ack")
        .then((a) => (ackAudio.current = a))
        .catch(() => {});
    }
  }, [standbyOn]);

  const toggleStandby = () => {
    unlockAudio(); // 応答音声を操作なしで再生できるようにする
    setWakeConfig({ ...wake, enabled: !wake.enabled })
      .then(setWake)
      .catch(fail);
  };

  const approvalId = approval?.id;
  useEffect(() => {
    if (!approvalId) return;
    void listenForAnswer(() => approvalRef.current?.id === approvalId);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [approvalId]);

  useEffect(() => {
    gate.current = { busy, needModel: needModel || needSandbox };
    abortRef.current = abortAll;
    working.current = busy || speaking || switching || !!approval || awake || mic === "starting" || mic === "recording" || mic === "transcribing";
    holdRef.current = (down) => {
      keyHeld.current = down;
      if (down) {
        clearTail(); // 離した直後に押し直したら、録音を続ける
        // 承認待ちの間は、押している間だけ録音して返答できる。常時待機の録音中なら、それに替えて始める
        const free = micNow.current === "idle" || micNow.current === "standby";
        if ((!busy || approvalRef.current) && !needModel && !needSandbox && free) void toggleMic(true);
      } else if (micNow.current === "recording") {
        stopAfterTail(); // 長押しを離して少し経ったら録音を止めて送信
      }
    };
    toggleRef.current = () => toggleMic();
  });

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!activeRef.current) return; // 他の画面ではAgentを通常の入力に使う
      const id = matchShortcut(e);
      if (id === "mic") {
        e.preventDefault();
        if (!e.repeat) holdRef.current(true); // 押しっぱなしのリピートで開始→即停止しない
      } else if (id === "abort" && working.current && !e.isComposing) {
        e.preventDefault();
        abortRef.current();
      }
    };
    // ボタンにフォーカスがあるとエージェントのkeyupでクリックが発火し、二重に切り替わるため止める
    const onKeyUp = (e: KeyboardEvent) => {
      if (e.code !== "Space") return;
      if (!activeRef.current) return;
      if (isPlainSpace(e)) e.preventDefault();
      holdRef.current(false);
    };
    const onBlur = () => holdRef.current(false); // 離したキーを取りこぼさない
    // 常時待機の応答は操作なしで鳴らすため、最初の操作で音声出力を有効にしておく
    const unlock = () => unlockAudio();
    window.addEventListener("keydown", onKey);
    window.addEventListener("keyup", onKeyUp);
    window.addEventListener("blur", onBlur);
    window.addEventListener("pointerdown", unlock);
    window.addEventListener("keydown", unlock);
    return () => {
      window.removeEventListener("pointerdown", unlock);
      window.removeEventListener("keydown", unlock);
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("keyup", onKeyUp);
    };
  }, []);

  const micHint = "Agent";

  const download = async () => {
    setDownloading(true);
    try {
      await downloadSttModel();
      setNeedModel(false);
      toast.success(t("音声認識モデルをインストールしました。"));
    } catch (e) {
      fail(e);
    } finally {
      setDownloading(false);
    }
  };

  const recording = mic === "recording";
  const phase: Phase =
    mic === "starting" ? "initializing"
    : recording ? "listening"
    : mic === "transcribing" ? "decoding"
    : busy ? "thinking"
    : speaking ? "speaking"
    : "standby";
  const status =
    mic === "starting" ? t("マイクを準備中…")
    : recording ? t("話し終えたら停止ボタンで送信")
    : awake ? t("どうぞ、お話しください")
    : phase === "standby" ? (standbyOn ? t("「{words}」と呼びかけてください", { words: wake.words[0] }) : t("マイクをオンにして話しかけてください"))
    : t("処理中…");

  const SMALL = "[@media(max-height:40rem),(max-width:40rem)]";
  return (
    <div className="relative flex h-full flex-col gap-3 p-3">
      <ChatNotices setup={setup} keyStatus={keyStatus} ttsReady={ttsReady} voiceReply={voiceReply} needModel={needModel} needSandbox={needSandbox} containerStartable={containerStartable} containerBusy={containerBusy} onStartContainer={() => void toggleContainer()} downloading={dlActive} progress={dlProgress} onDownload={download} />

      {/* 確認UIは、ウィンドウが小さくて会話を畳んでいるときも残す */}
      {switching && (
        <div className="flex shrink-0 justify-center">
          <SwitchConfirm listening={switchListening} onAnswer={answerSwitch} />
        </div>
      )}
      {/* 左: 会話のカード(広く使う)。右: 円形波形のカード。リーダーは、その下に台帳のカードを縦に並べる。狭いときは波形を会話の上に小さく出す */}
      <div className="flex min-h-0 flex-1 flex-col gap-3 md:flex-row">
        <section className={cn("hud-panel hud-accent min-h-0 min-w-0 flex-1 flex-col gap-3 px-5 py-4", `flex ${SMALL}:hidden`)} aria-label={t("会話")}>
          <p className="hud-heading shrink-0">{"// Conversation"}</p>
          <ChatThread
            turns={turns}
            busy={busy}
            activity={busy && !approval ? activity : null}
            speaking={speaking}
            onSpeak={ttsReady ? (text) => void speakOnDemand(text) : undefined}
            onStop={stopSpeech}
            onCancelTask={cancelTask}
            onCorrected={(text) => setTurns((ts) => ts.map((x, i) => (i === ts.length - 1 ? { ...x, user: text } : x)))}
          />
        </section>
        <div
          className={cn(
            "order-first flex h-44 shrink-0 flex-col gap-3 md:order-last md:h-auto md:w-[30%] md:max-w-[28rem] md:min-w-64",
            `${SMALL}:h-auto ${SMALL}:w-full ${SMALL}:max-w-none ${SMALL}:flex-1`,
          )}
        >
          {/* 利用できる幅・高さのうち小さい方に合わせて、円形波形を正方形で最大化する */}
          <section
            className={cn(
              "hud-panel hud-accent relative flex min-h-0 w-full items-center justify-center px-4 py-3 [container-type:size]",
              "flex-1 md:h-[45%] md:flex-none",
            )}
            aria-label={t("音声")}
          >
            <div className="relative size-[min(100cqmin,40rem)] @container">
              <VoiceOrb phase={phase} micActive={recording} thinking={busy} className="size-full" />
              <OrbReadout phase={phase} />
            </div>
          </section>
          {workspaceId === LEADER_ID ? (
            <LeaderLedger active={active} className={cn("hud-panel hud-accent hidden min-h-0 flex-1 md:flex", `${SMALL}:hidden`)} />
          ) : (
            <InfoCard
              turns={turns.length}
              modelLabel={models.find((m) => m.provider === route?.provider && m.id === route?.model)?.label}
              className={cn("hidden flex-1 md:flex", `${SMALL}:hidden`)}
            />
          )}
        </div>
      </div>

      {approval &&<ApprovalModal approval={approval} listening={listening} onAnswer={answerApproval} />}
      {dispatchDraft && (
        <DispatchDraftCard
          key={dispatchDraft.id}
          draft={dispatchDraft}
          onClose={() => setDispatchDraft(null)}
          className="absolute inset-x-6 bottom-36 z-20 mx-auto max-w-2xl"
        />
      )}

      {/* 下部: 操作ボタンと入力欄のカード */}
      <ChatComposer
        onNewConversation={() => startNewConversation(true)}
        container={
          sandboxOn && workspaceId !== LEADER_ID
            ? { running: !!sandboxReady, busy: containerBusy, disabled: !sandboxReady && !containerStartable, onToggle: () => void toggleContainer() }
            : undefined
        }
        standby={{ on: standbyOn, available: wake.words.length > 0, onToggle: toggleStandby }}
        mic={{
          state: mic,
          disabled: (busy && !approval) || mic === "starting" || mic === "transcribing" || needModel || needSandbox || (speaking && mic !== "recording"),
          hint: micHint,
          onToggle: () => void toggleMic(),
        }}
        speak={{
          on: speakReply,
          ready: ttsReady,
          onToggle: () => {
            if (speakReply) stopSpeech(); // 読み上げの途中でオフにしたら、その場で止める
            setVoiceReply((v) => !v);
          },
        }}
        draft={draft}
        onDraft={setDraft}
        onSubmit={submitDraft}
        placeholder={status}
        inputDisabled={busy || needModel || needSandbox}
        sendDisabled={!draft.trim() || busy || needModel || needSandbox || mic !== "idle"}
        busy={busy}
        onStop={abortAll}
        model={
          sandboxOn
            ? { kind: "sandbox", models: sandboxModels?.models ?? [], selected: sandboxModels?.selected ?? sandboxModels?.models[0] ?? null, onChange: changeSandboxModel }
            : { kind: "app", route, models, keyStatus, onChange: changeRoute }
        }
      />
    </div>
  );
}
