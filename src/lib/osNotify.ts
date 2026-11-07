import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";

const KEY = "osNotifications";

/** アプリが見えていない間の、OS(macOS)の通知を使うか。この端末の設定で、既定はオン。 */
export function osNotificationsEnabled(): boolean {
  try {
    return localStorage.getItem(KEY) !== "0";
  } catch {
    return true;
  }
}

export function setOsNotificationsEnabled(on: boolean) {
  try {
    localStorage.setItem(KEY, on ? "1" : "0");
  } catch {}
}

/** アプリが背面にある、またはウィンドウが閉じて(隠れて)いるか。 */
export const appInBackground = () => document.hidden || !document.hasFocus();

/** 通知の許可を得る(初回はOSのダイアログが出る)。許可されていれば true。 */
export async function ensureOsPermission(): Promise<boolean> {
  try {
    return (await isPermissionGranted()) || (await requestPermission()) === "granted";
  } catch {
    return false;
  }
}

/** アプリが背面のときだけ、設定がオンなら、OSの通知を出す。失敗しても、画面のトーストがあるので黙って諦める。 */
export async function notifyOsIfBackground(title: string, body?: string) {
  if (!osNotificationsEnabled() || !appInBackground()) return;
  try {
    if (await isPermissionGranted()) sendNotification({ title, body });
  } catch {}
}
