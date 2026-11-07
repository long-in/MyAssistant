import { getGlobalPrefs, setGlobalLanguage } from "@/lib/api";
import { currentLang, setLang } from "@/lib/i18n";
import { syncOsNotifications } from "@/lib/osNotify";

/**
 * 全体設定の表示言語とOS通知を、画面へ取り込む。アプリの起動時に1回行う。
 * 全体設定が未保存の項目は、この端末の前回の値を初期値として全体設定へ移す。
 */
export async function syncGlobalPrefs() {
  const { language } = await getGlobalPrefs();
  if (language === null) await setGlobalLanguage(currentLang());
  else if (language !== currentLang()) setLang(language);
  await syncOsNotifications();
}
