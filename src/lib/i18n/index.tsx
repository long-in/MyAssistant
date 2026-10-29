"use client";

import { useSyncExternalStore } from "react";
import { EN } from "./en";

export type Lang = "ja" | "en";

export const LANGS: { value: Lang; label: string }[] = [
  { value: "ja", label: "日本語" },
  { value: "en", label: "English" },
];

const STORAGE_KEY = "language";

/** 保存済みの言語。未設定ならOSの言語から決める。 */
function detect(): Lang {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved === "ja" || saved === "en") return saved;
  } catch {}
  return typeof navigator !== "undefined" && navigator.language.startsWith("ja") ? "ja" : "en";
}

let current: Lang | null = null;
const listeners = new Set<() => void>();

const getLang = (): Lang => (current ??= detect());

export function setLang(lang: Lang) {
  current = lang;
  try {
    localStorage.setItem(STORAGE_KEY, lang);
  } catch {}
  document.documentElement.lang = lang;
  listeners.forEach((l) => l());
}

const subscribe = (cb: () => void) => {
  listeners.add(cb);
  return () => listeners.delete(cb);
};

/** 日本語の原文をキーに翻訳する。`{name}` は vars で置換。辞書にない文字列は原文のまま返す。 */
export function tr(ja: string, vars?: Record<string, string | number>): string {
  let s = typeof window !== "undefined" && getLang() === "en" ? (EN[ja] ?? ja) : ja;
  if (vars) for (const [k, v] of Object.entries(vars)) s = s.replaceAll(`{${k}}`, String(v));
  return s;
}

/** 現在の言語。SSR・ハイドレーション時は "ja"。 */
export function useLang(): Lang {
  return useSyncExternalStore(subscribe, getLang, () => "ja");
}

/** 言語の変更で再描画される翻訳関数。 */
export function useT() {
  useLang();
  return tr;
}
