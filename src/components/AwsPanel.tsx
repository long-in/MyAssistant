"use client";

import { useCallback, useEffect, useState } from "react";
import { ChevronDown, Download, Eye, EyeOff, FolderOpen, KeyRound, Loader2, Plug, Plus, Trash2, X } from "lucide-react";
import { StatusPill, UsedBy, linkCardClass } from "@/components/GlobalPanels";
import { toast } from "sonner";
import { useT } from "@/lib/i18n";
import {
  type AwsStatus,
  addAwsModel,
  deleteApiKey,
  type AwsProfile,
  fetchAwsModels,
  getAwsFiles,
  getAwsStatus,
  previewAwsConfig,
  revealSecretsFile,
  removeAwsModel,
  reportError,
  saveAwsConfig,
  selectAwsProfile,
  testAwsConnection,
} from "@/lib/api";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";

const CONFIG_PLACEHOLDER = `[default]
region = us-east-1

[profile bedrock]
role_arn = arn:aws:iam::123456789012:role/BedrockUser
source_profile = base
mfa_serial = arn:aws:iam::111111111111:mfa/me`;

const CREDENTIALS_PLACEHOLDER = `[base]
aws_access_key_id = AKIA...
aws_secret_access_key = ...`;

/**
 * Amazon Bedrock(Claude)の接続設定。`~/.aws/config`と`~/.aws/credentials`の中身をそのまま貼り付けて登録する。
 * 登録後は「更新」を開くと保存済みの内容が入った状態で編集できる。credentialsは既定で伏せ字にする。
 */
export default function AwsPanel({ onChanged, usedBy = [] }: { onChanged: () => void; usedBy?: string[] }) {
  const t = useT();
  const [status, setStatus] = useState<AwsStatus | null>(null);
  const [open, setOpen] = useState(false);
  const [config, setConfig] = useState("");
  const [credentials, setCredentials] = useState("");
  const [reveal, setReveal] = useState(false);
  const [preview, setPreview] = useState<AwsProfile[]>([]);
  const [chosen, setChosen] = useState("");
  const [modelInput, setModelInput] = useState("");
  const [fetched, setFetched] = useState<string[] | null>(null);
  const [busy, setBusy] = useState<"save" | "test" | "delete" | "model" | "fetch" | null>(null);

  const refresh = useCallback(
    () =>
      getAwsStatus()
        .then(setStatus)
        .catch((e) => toast.error(reportError(e))),
    [],
  );
  useEffect(() => {
    refresh();
  }, [refresh]);

  // 貼り付け中の内容から、使えるプロファイルを読み取って選ばせる
  useEffect(() => {
    if (!config.trim() && !credentials.trim()) return;
    const timer = setTimeout(() => {
      previewAwsConfig(config, credentials)
        .then((list) => {
          const names = list.filter((p) => p.usable).map((p) => p.name);
          setPreview(list);
          // 候補が1つならそれ。複数あるときは、いま選んでいるものが残っていれば維持し、無ければ選ばせる
          setChosen((cur) => (names.length === 1 ? names[0] : names.includes(cur) ? cur : names.find((n) => n === status?.selected) ?? ""));
        })
        .catch(() => setPreview([]));
    }, 300);
    return () => clearTimeout(timer);
  }, [config, credentials, status?.selected]);

  // 更新を開いたら、保存済みの内容を読み込んで編集できるようにする(閉じたら破棄)
  useEffect(() => {
    if (!open) return;
    let alive = true;
    getAwsFiles()
      .then((f) => {
        if (!alive) return;
        setConfig(f.config);
        setCredentials(f.credentials);
      })
      .catch((e) => toast.error(reportError(e)));
    return () => {
      alive = false;
      setReveal(false);
    };
  }, [open]);

  const run = async (kind: NonNullable<typeof busy>, fn: () => Promise<AwsStatus | void>, ok?: string) => {
    setBusy(kind);
    try {
      const next = await fn();
      if (next) setStatus(next);
      else await refresh();
      onChanged();
      if (ok) toast.success(ok);
      return true;
    } catch (e) {
      toast.error(reportError(e));
      return false;
    } finally {
      setBusy(null);
    }
  };

  const registered = !!status?.registered;
  const usable = (status?.profiles ?? []).filter((p) => p.usable);
  const current = usable.find((p) => p.name === status?.selected);
  const pasted = !!config.trim() || !!credentials.trim();
  const labelOf = (p: AwsProfile) =>
    p.role ? `${p.name} (${p.mfa ? "AssumeRole + MFA" : "AssumeRole"})` : p.name;
  const items = usable.map((p) => ({ value: p.name, label: labelOf(p) }));
  const unusable = pasted ? preview.filter((p) => !p.usable && p.reason) : [];
  const candidates = (pasted ? preview : []).filter((p) => p.usable).map((p) => ({ value: p.name, label: labelOf(p) }));

  const expires = status?.session_expires_at ? new Date(status.session_expires_at * 1000) : null;

  return (
    <div className={linkCardClass(registered)}>
      <div className="flex items-center gap-3">
        <span
          aria-hidden
          className={cn(
            "size-2 shrink-0 rounded-full border",
            registered ? "border-primary bg-primary shadow-[0_0_8px_var(--primary)]" : "border-muted-foreground/50",
          )}
        />
        <div className="min-w-0 flex-1">
          <div className="text-sm font-medium">Amazon Bedrock (Claude)</div>
          <div className="text-xs text-muted-foreground">{t("AWSのスイッチロール(MFA対応)で接続")}</div>
        </div>
        <span className="hidden sm:inline" aria-live="polite">
          {registered && <StatusPill on>linked</StatusPill>}
        </span>
        {registered && (
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            aria-label={t("削除")}
            disabled={busy !== null}
            onClick={() => run("delete", () => deleteApiKey("bedrock"))}
          >
            <Trash2 />
          </Button>
        )}
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          aria-label={t("保存先をFinderで開く")}
          title={t("保存先をFinderで開く")}
          onClick={() => revealSecretsFile().catch((e) => toast.error(reportError(e)))}
        >
          <FolderOpen />
        </Button>
        <Button type="button" variant="outline" size="sm" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
          {registered ? t("更新") : t("接続")}
          <ChevronDown className={cn("transition-transform", open && "rotate-180")} />
        </Button>
      </div>

      <UsedBy features={usedBy} linked={registered} />

      {registered && (
        <div className="mt-3 flex flex-wrap items-center gap-2 pl-5">
          <Select
            items={items}
            value={status?.selected ?? ""}
            onValueChange={(v) => v && run("save", () => selectAwsProfile(v as string))}
          >
            <SelectTrigger className="w-64" aria-label={t("AWSプロファイル")}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {items.map((i) => (
                <SelectItem key={i.value} value={i.value}>{i.label}</SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={busy !== null}
            onClick={() => run("test", testAwsConnection, t("Bedrockに接続できました。"))}
          >
            {busy === "test" ? <Loader2 className="animate-spin" /> : <Plug />}
            {busy === "test" ? t("確認中…") : t("接続テスト")}
          </Button>
          <span className="text-xs text-muted-foreground">
            {current?.region && <>{current.region} · </>}
            {expires
              ? t("一時認証の期限: {time}", { time: expires.toLocaleTimeString() })
              : current?.role
                ? t("一時認証: 未取得(初回の利用時に取得)")
                : ""}
          </span>
        </div>
      )}

      {registered && (
        <div className="mt-3 flex flex-col gap-2 pl-5">
          <div className="text-xs text-muted-foreground">{t("使うモデル(モデルID・推論プロファイルID・ARN)")}</div>
          {status.models.length === 0 && (
            <div className="text-xs text-destructive">{t("モデルが未登録です。追加するとモデル選択に並びます。")}</div>
          )}
          <ul className="flex flex-col gap-1">
            {status.models.map((m) => (
              <li key={m} className="flex items-center gap-2 rounded-md border px-2 py-1">
                <span className="min-w-0 flex-1 break-all font-mono text-xs">{m}</span>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-xs"
                  aria-label={t("削除")}
                  disabled={busy !== null}
                  onClick={() => run("model", () => removeAwsModel(m))}
                >
                  <X />
                </Button>
              </li>
            ))}
          </ul>
          <form
            className="flex gap-2"
            onSubmit={async (e) => {
              e.preventDefault();
              if (await run("model", () => addAwsModel(modelInput))) setModelInput("");
            }}
          >
            <Input
              autoComplete="off"
              spellCheck={false}
              className="font-mono text-xs"
              placeholder="us.anthropic.claude-sonnet-4-5-20250929-v1:0"
              value={modelInput}
              onChange={(e) => setModelInput(e.target.value)}
            />
            <Button type="submit" variant="outline" disabled={busy !== null || !modelInput.trim()}>
              <Plus />
              {t("追加")}
            </Button>
          </form>
          <div className="flex flex-wrap items-center gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={busy !== null}
              onClick={async () => {
                setBusy("fetch");
                try {
                  setFetched(await fetchAwsModels());
                } catch (e) {
                  toast.error(reportError(e));
                } finally {
                  setBusy(null);
                }
              }}
            >
              {busy === "fetch" ? <Loader2 className="animate-spin" /> : <Download />}
              {busy === "fetch" ? t("取得中…") : t("Bedrockからglobalのモデルを取得")}
            </Button>
            {fetched && fetched.filter((m) => !status.models.includes(m)).length === 0 && (
              <span className="text-xs text-muted-foreground">
                {fetched.length === 0 ? t("globalのClaudeモデルが見つかりませんでした。") : t("取得したモデルはすべて登録済みです。")}
              </span>
            )}
          </div>
          {fetched && fetched.filter((m) => !status.models.includes(m)).length > 0 && (
            <div className="flex flex-wrap items-center gap-1.5">
              <span className="text-xs text-muted-foreground">{t("取得結果:")}</span>
              {fetched
                .filter((m) => !status.models.includes(m))
                .map((m) => (
                  <Button
                    key={m}
                    type="button"
                    variant="outline"
                    size="xs"
                    className="font-mono"
                    disabled={busy !== null}
                    onClick={() => run("model", () => addAwsModel(m))}
                  >
                    <Plus />
                    {m}
                  </Button>
                ))}
            </div>
          )}
          {status.suggested_models.length > 0 && (
            <div className="flex flex-wrap items-center gap-1.5">
              <span className="text-xs text-muted-foreground">{t("候補:")}</span>
              {status.suggested_models.map((m) => (
                <Button
                  key={m}
                  type="button"
                  variant="outline"
                  size="xs"
                  className="font-mono"
                  disabled={busy !== null}
                  onClick={() => run("model", () => addAwsModel(m))}
                >
                  <Plus />
                  {m}
                </Button>
              ))}
            </div>
          )}
          <p className="text-xs text-muted-foreground">
            {t("実在のIDはAWSコンソール、または aws bedrock list-inference-profiles で確認できます。「接続テスト」は先頭のモデルで確認します。")}
          </p>
        </div>
      )}

      {open && (
        <form
          className="mt-3 flex flex-col gap-3 pl-5"
          onSubmit={async (e) => {
            e.preventDefault();
            const ok = await run("save", () => saveAwsConfig(config, credentials, chosen), t("AWSの設定を保存しました。"));
            if (ok) {
              setConfig("");
              setCredentials("");
              setPreview([]);
              setChosen("");
              setOpen(false);
            }
          }}
        >
          <label className="flex flex-col gap-1 text-xs text-muted-foreground">
            ~/.aws/config
            <Textarea
              autoComplete="off"
              spellCheck={false}
              rows={6}
              className="font-mono text-xs"
              placeholder={CONFIG_PLACEHOLDER}
              value={config}
              onChange={(e) => setConfig(e.target.value)}
            />
          </label>
          <label className="flex flex-col gap-1 text-xs text-muted-foreground">
            <span className="flex items-center gap-1">
              ~/.aws/credentials
              <Button
                type="button"
                variant="ghost"
                size="xs"
                aria-pressed={reveal}
                onClick={() => setReveal((r) => !r)}
              >
                {reveal ? <EyeOff /> : <Eye />}
                {reveal ? t("隠す") : t("表示")}
              </Button>
            </span>
            <Textarea
              autoComplete="off"
              spellCheck={false}
              rows={4}
              className={cn("font-mono text-xs", !reveal && "[-webkit-text-security:disc]")}
              placeholder={CREDENTIALS_PLACEHOLDER}
              value={credentials}
              onChange={(e) => setCredentials(e.target.value)}
            />
          </label>
          <p className="text-xs text-muted-foreground">
            {t("それぞれの中身をそのまま貼り付けます。登録済みの場合は保存済みの内容が入っているので、編集して保存してください。credentialsは既定で伏せ字です。")}
          </p>
          {pasted && (
            <div className="flex flex-col gap-1 text-xs text-muted-foreground">
              {t("使うプロファイル")}
              {candidates.length > 0 ? (
                <Select items={candidates} value={chosen || null} onValueChange={(v) => v && setChosen(v as string)}>
                  <SelectTrigger className="w-72" aria-label={t("使うプロファイル")}>
                    <SelectValue placeholder={t("選択してください")} />
                  </SelectTrigger>
                  <SelectContent>
                    {candidates.map((i) => (
                      <SelectItem key={i.value} value={i.value}>{i.label}</SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              ) : (
                <span className="text-destructive">
                  {t("使えるプロファイルがありません。アクセスキーを持つプロファイル、またはそれを source_profile にした role_arn のプロファイルが必要です。")}
                </span>
              )}
              {unusable.length > 0 && (
                <div className="mt-1">
                  <div>{t("選べないプロファイル")}</div>
                  <ul className="mt-0.5 flex flex-col gap-0.5">
                    {unusable.map((p) => (
                      <li key={p.name} className="break-words">
                        <span className="font-mono text-foreground">{p.name}</span>
                        <span className="font-mono"> — {p.reason}</span>
                      </li>
                    ))}
                  </ul>
                </div>
              )}
            </div>
          )}
          <div>
            <Button type="submit" disabled={busy !== null || !pasted || !chosen}>
              {busy === "save" ? <Loader2 className="animate-spin" /> : <KeyRound />}
              {busy === "save" ? t("確認中…") : t("検証して保存")}
            </Button>
          </div>
        </form>
      )}
    </div>
  );
}
