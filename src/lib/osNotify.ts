import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { getGlobalPrefs, setGlobalOsNotifications } from "@/lib/api";

const KEY = "osNotifications";

/** 全体設定の値の写し。同期的に読めるよう、アプリの起動時に全体設定から読み込む。読み込むまでは、この端末の前回の保存値を使う。 */
let current: boolean | null = null;

function cached(): boolean {
  if (current !== null) return current;
  try {
    return localStorage.getItem(KEY) !== "0";
  } catch {
    return true;
  }
}

/** アプリが見えていない間の、OS(macOS)の通知を使うか。全体設定で、既定はオン。 */
export const osNotificationsEnabled = cached;

function remember(on: boolean) {
  current = on;
  try {
    localStorage.setItem(KEY, on ? "1" : "0");
  } catch {}
}

/** 全体設定へ保存する。 */
export async function setOsNotificationsEnabled(on: boolean) {
  await setGlobalOsNotifications(on);
  remember(on);
}

/** 全体設定の値を読み込む。未保存なら、この端末の前回の保存値を初期値として全体設定へ移す。 */
export async function syncOsNotifications() {
  const { os_notifications } = await getGlobalPrefs();
  if (os_notifications === null) await setGlobalOsNotifications(cached());
  remember(os_notifications ?? cached());
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
