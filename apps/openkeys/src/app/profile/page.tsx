import { cookies } from "next/headers";
import { KeyLogin } from "@/components/key-login";
import { KeyProfile } from "@/components/key-profile";
import { loadUsageByViewToken } from "@/lib/keys";
import { USAGE_SESSION_COOKIE } from "@/lib/usage-session";

export const dynamic = "force-dynamic";
export const metadata = { title: "Профиль ключа — OpenKeys" };

/**
 * Тот же профиль, но вход по самому ключу: проверенный ключ кладёт в HttpOnly-куку
 * ссылку на профиль, поэтому обновление страницы не требует вводить ключ заново,
 * а секрет в браузере не остаётся.
 */
export default async function ProfilePage() {
  const store = await cookies();
  const viewToken = store.get(USAGE_SESSION_COOKIE)?.value;
  if (!viewToken) return <KeyLogin />;

  const view = await loadUsageByViewToken(viewToken);
  if (!view) return <KeyLogin />;

  return <KeyProfile view={view} showSignOut />;
}
