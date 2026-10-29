"use client";

import { useT } from "@/lib/i18n";
import { useEffect, useRef, useState } from "react";
import { Download, Ear, Loader2, MessageSquarePlus, Mic, Square, Volume2, VolumeX } from "lucide-react";
import { toast } from "sonner";
import ModelSelector from "./ModelSelector";
import { Button } from "@/components/ui/button";
import { VoiceOrb } from "./Waveforms";
import UsageHud from "./UsageHud";
import Subtitles from "./Subtitles";
import ApprovalModal, { type Approval, type Listening } from "./ApprovalModal";
import SwitchConfirm from "./SwitchConfirm";
import ExecutionLog, { applyAgentEvent, emptyLog, type LogState } from "./ExecutionLog";
import ExchangeLog, { type Turn } from "./ExchangeLog";
import OrbReadout, { type Phase } from "./OrbReadout";
import { cn } from "@/lib/utils";
import { isPlainSpace, matchShortcut } from "@/lib/shortcuts";
import { judgeAnswer } from "@/lib/voiceApproval";
import { detectSwitch } from "@/lib/voiceCommands";
import {
  type AgentEvent,
  type KeyStatus,
  type ModelInfo,
  type Route,
  type UsageRow,
  DEFAULT_TUNING,
  type Tuning,
  type WakeConfig,
  agentApprove,
  agentCancel,
  cancelRecording,
  downloadSttModel,
  getSttEngine,
  getKeyStatus,
  getTuning,
  getWakeConfig,
  getUsageThisMonth,
  isSpeaking,
  listModels,
  loadRouting,
  playBase64Mp3,
  recordingWaveform,
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
  ttsKeyRegistered,
  voicePhraseAudio,
  waitForSpeechEnd,
} from "@/lib/api";

// 録音の待ち時間(スペースを離したあとの猶予・声が入らず終える時間・話し終わりの無音)は、設定ページで変えられる(`tuning`)。

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
};

/** `standby`: 常時待機が起動ワードを聞き取るために録音している。手動の操作が始まれば譲る。 */
type Mic = "idle" | "starting" | "recording" | "transcribing" | "standby";

/** 録音中の入力を見て話し終えるのを待った結果。`lost`は手動操作などで録音の持ち主が変わったこと。 */
type Capture = "spoke" | "silent" | "lost";
/** 録音して文字起こしした結果。`retry`は聞き取れなかった、`stop`はマイクが使えない。 */
type Heard = { text: string } | "retry" | "stop";

/** 高さ40rem未満または幅40rem未満のウィンドウでは、波形以外の補助表示を隠す。 */
const HIDE_WHEN_SMALL = "[@media(max-height:40rem),(max-width:40rem)]:hidden";
const HIDE_WHEN_SMALL_FLEX = "flex [@media(max-height:40rem),(max-width:40rem)]:hidden";
const SMALL_COLLAPSE_SUBTITLES = "h-[clamp(9rem,26vh,14rem)] [@media(max-height:40rem),(max-width:40rem)]:h-auto";

/** 現在の会話のセッション。設定画面などへ移動して`Chat`が作り直されても引き継ぐ(リロード・再起動で新しくなる)。 */
let currentSession = { id: crypto.randomUUID() };

export default function Chat() {
  const t = useT();
  // 会話の切り替えは、ユーザーが明示したときだけ。コマンド実行の依頼・承認や画面移動では切り替えない(再起動後は、バックエンドが前回までの文脈をMarkdownから復元する)
  const session = useRef(currentSession);
  const [turn, setTurn] = useState<Turn | null>(null);
  const [busy, setBusy] = useState(false);
  const [activity, setActivity] = useState<string | null>(null);
  const [approval, setApproval] = useState<Approval | null>(null);
  // 音声の返答の待ち受けは非同期で動くので、最新の承認要求をrefでも持つ
  const approvalRef = useRef<Approval | null>(null);
  const [listening, setListening] = useState<Listening>("idle");
  const [log, setLog] = useState<LogState>(emptyLog);
  const [route, setRoute] = useState<Route | null>(null);
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
  const [usage, setUsage] = useState<UsageRow[]>([]);
  const [needModel, setNeedModel] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const [ttsReady, setTtsReady] = useState(false);
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
  const tuning = useRef<Tuning>(DEFAULT_TUNING);
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
  /** スペースを離してから録音を止めるまでの待ちタイマー。語尾が切れないよう少し録り続ける。 */
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

  useEffect(() => {
    Promise.all([listModels(), loadRouting(), getKeyStatus(), ttsKeyRegistered(), getWakeConfig(), getTuning()])
      .then(([m, r, k, t, w, u]) => {
        tuning.current = u;
        setModels(m);
        setRoute(r);
        setKeyStatus(k);
        setTtsReady(t);
        setWake(w);
      })
      .catch(fail);
  }, []);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    const id = setInterval(() => setSpeaking(isSpeaking()), 150);
    return () => clearInterval(id);
  }, []);

  const refreshUsage = () => getUsageThisMonth().then(setUsage).catch(() => {});
  useEffect(() => {
    refreshUsage();
  }, []);

  const changeRoute = (r: Route) => {
    setRoute(r);
    saveRouting(r).catch(fail);
  };

  const speak = async (text: string) => {
    try {
      await playBase64Mp3(await synthesizeSpeech(text), text);
    } catch (e) {
      fail(e);
    }
  };

  const showApproval = (a: Approval | null) => {
    approvalRef.current = a;
    setApproval(a);
  };

  const onAgentEvent = (e: AgentEvent) => {
    setLog((l) => applyAgentEvent(l, e));
    if (e.type === "tool_call") setActivity(t(TOOL_ACTIVITY[e.name] ?? "ファイルを操作しています…"));
    else if (e.type === "tool_result") setActivity(null);
    else if (e.type === "approval") showApproval(e);
  };

  const answerApproval = (approved: boolean) => {
    const a = approvalRef.current;
    if (!a) return;
    showApproval(null);
    agentApprove(a.id, approved).catch(fail);
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
    agentCancel().catch(fail);
  };

  /** 新しい会話を始める。これまでの文脈は引き継がず、実行中の処理と読み上げは止める。 */
  const startNewConversation = (interrupt: boolean) => {
    currentSession = { id: crypto.randomUUID() };
    session.current = currentSession;
    if (interrupt) resetEpoch.current++; // 会話中のループは自分で続けるので、止めるのは外からの操作のときだけ
    stopSpeech();
    if (gate.current.busy || approvalRef.current) cancelTask();
    setTurn(null);
    setLog(emptyLog);
    toast.success(t("新しい会話を始めました。"));
  };

  /** 依頼を送る。`spoke`: 返答を読み上げた、`replied`: 返答は画面のみ、`none`: 返答なし。 */
  const submit = async (raw: string): Promise<"spoke" | "replied" | "none"> => {
    const text = raw.trim();
    if (!text || busy) return "none";
    unlockAudio(); // 操作のタイミングで有効化し、応答の自動再生を可能にする
    const sessionId = session.current.id;
    const userAt = new Date();
    setLog(emptyLog);
    setBusy(true);
    setTurn({ user: text, userAt });
    let result: "spoke" | "replied" | "none" = "none";
    try {
      if (!route) throw new Error(t("使えるモデルがありません。設定でAPIキーを登録してください。"));
      const r = await sendChat(sessionId, text, route, onAgentEvent);
      refreshUsage();
      if (session.current.id !== sessionId) return "none"; // 待っている間に新しい会話へ切り替わった
      const replied = { user: text, userAt, reply: r.content, replyAt: new Date(), via: r.model };
      result = "replied";
      if (speakReply) {
        // 字幕を読み上げの再生位置に合わせるため、音声を作って再生を始めてから返答を表示する
        const audio = await synthesizeSpeech(r.content).catch((e) => {
          fail(e);
          return null;
        });
        if (session.current.id !== sessionId) return "none";
        if (audio && speakReplyRef.current) await playBase64Mp3(audio, r.content).catch(fail);
        result = "spoke";
      }
      setTurn(replied);
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
      setActivity(null);
      showApproval(null);
    }
    return result;
  };

  /** `hold`: スペース長押しでの開始。開始処理中にキーが離されていたら、録音できた時点でそのまま停止する。 */
  const toggleMic = async (hold = false) => {
    // 開始・停止処理の最中の再入力(キーのリピートや連打)は無視し、録音を勝手に止めない
    const state = micNow.current;
    if (micLock.current || state === "starting" || state === "transcribing") return;
    micLock.current = true;
    unlockAudio();
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
    if (text) {
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
  const handleWake = async (rest: string, alive: () => boolean) => {
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
    gate.current = { busy, needModel };
    holdRef.current = (down) => {
      keyHeld.current = down;
      if (down) {
        clearTail(); // 離した直後に押し直したら、録音を続ける
        // 承認待ちの間は、押している間だけ録音して返答できる。常時待機の録音中なら、それに替えて始める
        const free = micNow.current === "idle" || micNow.current === "standby";
        if ((!busy || approvalRef.current) && !needModel && free) void toggleMic(true);
      } else if (micNow.current === "recording") {
        stopAfterTail(); // 長押しを離して少し経ったら録音を止めて送信
      }
    };
    toggleRef.current = () => toggleMic();
  });

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const id = matchShortcut(e);
      if (id === "mic") {
        e.preventDefault();
        if (!e.repeat) holdRef.current(true); // 押しっぱなしのリピートで開始→即停止しない
      }
    };
    // ボタンにフォーカスがあるとスペースのkeyupでクリックが発火し、二重に切り替わるため止める
    const onKeyUp = (e: KeyboardEvent) => {
      if (e.code !== "Space") return;
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

  const micHint = "Space";

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

  return (
    <div className="flex h-full flex-col">
      {needModel && (
        <div className="mx-6 mt-4 flex items-center justify-between gap-3 rounded-lg border bg-muted/40 p-3 text-sm">
          <span>{t("音声入力にはローカル音声認識モデル(約470MB)のダウンロードが必要です。")}</span>
          <Button size="sm" disabled={downloading} onClick={download}>
            {downloading ? <Loader2 className="animate-spin" /> : <Download />}
            {downloading ? t("ダウンロード中…") : t("ダウンロード")}
          </Button>
        </div>
      )}

      {/* 中央: 読み上げ音声の円形波形 */}
      <div className="relative flex min-h-0 flex-1 flex-col items-center justify-center gap-4 px-6 py-4">
        <div className={HIDE_WHEN_SMALL}>
          <UsageHud rows={usage} />
        </div>
        {/* 利用できる幅・高さのうち小さい方に合わせて、円形波形を正方形で最大化する */}
        <div className="flex min-h-0 w-full flex-1 items-center justify-center [container-type:size]">
          <div className="relative size-[min(100cqmin,40rem)] @container">
            <VoiceOrb phase={phase} micActive={recording} thinking={busy} className="size-full" />
            <OrbReadout phase={phase} />
          </div>
        </div>
        {/* 左上: 直前の会話と、その下に実行ログ */}
        <div className={cn("pointer-events-none absolute top-4 left-4 z-10 w-72 max-w-[calc(100%-2rem)] flex-col gap-3", HIDE_WHEN_SMALL_FLEX)}>
          <ExchangeLog turn={turn} pending={busy} canSpeak={ttsReady} onSpeak={speak} />
          <ExecutionLog log={log} busy={busy} onCancel={cancelTask} onClose={() => setLog(emptyLog)} />
        </div>
        {/* 字幕は固定高の領域に収め、長文は領域内でスクロールさせる(円形波形の大きさを変えない) */}
        {/* ウィンドウが小さいときは字幕などを畳み、円形波形を大きく保つ(確認UIだけは残す) */}
        <div className={cn("flex w-full shrink-0 flex-col items-center justify-end gap-1.5", SMALL_COLLAPSE_SUBTITLES)}>
          <div className={cn("w-full flex-col items-center gap-1.5", HIDE_WHEN_SMALL_FLEX)}>
            {busy && activity && !approval && (
              <p className="flex shrink-0 items-center gap-2 text-xs text-muted-foreground">
                <Loader2 className="size-3.5 animate-spin" /> {activity}
              </p>
            )}
          </div>
          {switching && <SwitchConfirm listening={switchListening} onAnswer={answerSwitch} />}
          <div className={cn("flex min-h-0 w-full flex-1 flex-col items-center justify-end", HIDE_WHEN_SMALL)}>
            <Subtitles turn={turn} pending={busy} onCorrected={(text) => setTurn((t) => t && { ...t, user: text })} />
          </div>
        </div>
      </div>

      {approval && <ApprovalModal approval={approval} listening={listening} onAnswer={answerApproval} />}

      {/* 下部: マイク入力の横長波形 */}
      <div className="border-t border-primary/20 px-6 py-4">
        <div className="mx-auto mb-3 flex max-w-3xl items-center justify-between gap-3">
          <ModelSelector route={route} models={models} keyStatus={keyStatus} onChange={changeRoute} />
          <div className="flex items-center gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              aria-pressed={speakReply}
              disabled={!ttsReady}
              title={ttsReady ? t("応答の読み上げのオン/オフ") : t("設定でGoogle Cloud TTSのキーを登録してください")}
              onClick={() => {
                if (speakReply) stopSpeech(); // 読み上げの途中でオフにしたら、その場で止める
                setVoiceReply((v) => !v);
              }}
            >
              {speakReply ? <Volume2 /> : <VolumeX />}
              {speakReply ? t("読み上げ") : t("ミュート")}
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              title={t("これまでの会話の流れを引き継がず、新しい会話を始める")}
              onClick={() => startNewConversation(true)}
            >
              <MessageSquarePlus />
              {t("新しい会話")}
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              aria-pressed={standbyOn}
              disabled={!wake.words.length}
              title={wake.words.length ? t("呼びかけで起動する常時待機のオン/オフ") : t("設定で起動ワードを登録してください")}
              onClick={toggleStandby}
            >
              <Ear />
              {standbyOn ? t("待機中") : t("待機オフ")}
            </Button>
          </div>
        </div>
        <div className="mx-auto flex max-w-3xl items-center gap-4">
          <Button
            type="button"
            size="icon-lg"
            variant={recording ? "destructive" : "outline"}
            aria-label={recording ? t("録音を停止して送信") : t("音声入力")}
            title={t("{recording} ({micHint}を押している間)", { recording: recording ? "録音を停止して送信" : "音声入力", micHint })}
            aria-pressed={recording}
            disabled={(busy && !approval) || mic === "starting" || mic === "transcribing" || needModel}
            className={cn("shrink-0", recording && "animate-pulse")}
            onClick={() => void toggleMic()}
          >
            {mic === "starting" || mic === "transcribing" ? <Loader2 className="animate-spin" /> : recording ? <Square /> : <Mic />}
          </Button>
          <span className="hud-label min-w-0 flex-1 text-[0.65rem]">{status}</span>
        </div>
      </div>
    </div>
  );
}
