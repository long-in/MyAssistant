"use client";

import { useSyncExternalStore } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  PERSONAL_ID,
  WORKSPACE_CHANGED_EVENT,
  getActiveWorkspace,
  listSpaceColors,
  listSpaces,
  switchWorkspace,
  type ActiveWorkspace,
  type Space,
} from "@/lib/api";

/** ワークスペースの現在値。起動直後は読み込み前(`ready`が偽)で、パーソナルとして扱う。 */
export type WorkspaceState = {
  ready: boolean;
  active: ActiveWorkspace;
  spaces: Space[];
  palette: string[];
  /** 選択中のワークスペースの設定を外から書き換えたときに増える(設定を読み直すための印)。 */
  settingsVersion: number;
};

const PERSONAL: ActiveWorkspace = { id: PERSONAL_ID, space: null };
const INITIAL: WorkspaceState = { ready: false, active: PERSONAL, spaces: [], palette: [], settingsVersion: 0 };

let state = INITIAL;
const listeners = new Set<() => void>();

function set(next: Partial<WorkspaceState>) {
  state = { ...state, ...next };
  listeners.forEach((l) => l());
}

/** バックエンドから現在値を読み直す。 */
export async function refreshWorkspace() {
  const [active, spaces, palette] = await Promise.all([getActiveWorkspace(), listSpaces(), listSpaceColors()]);
  set({ ready: true, active, spaces, palette });
}

/** 選択中のワークスペースの設定が、コピーなどで書き換わったことを知らせる。 */
export function bumpSettingsVersion() {
  set({ settingsVersion: state.settingsVersion + 1 });
}

/** ワークスペースを切り替える。切り替えの前後で会話などを止めるのは、`id`の変化を見る側が行う。 */
export async function switchTo(id: string) {
  if (id === state.active.id) return;
  const active = await switchWorkspace(id);
  set({ active });
}

let started = false;

/** 起動時に1回呼ぶ。バックエンドからの変更通知(名称変更・削除での切り戻し)も取り込む。 */
export function startWorkspaceSync() {
  if (started) return;
  started = true;
  void refreshWorkspace().catch(() => set({ ready: true }));
  void listen(WORKSPACE_CHANGED_EVENT, () => void refreshWorkspace().catch(() => {}));
}

const subscribe = (cb: () => void) => {
  listeners.add(cb);
  return () => listeners.delete(cb);
};

export function useWorkspace(): WorkspaceState {
  return useSyncExternalStore(subscribe, () => state, () => INITIAL);
}
