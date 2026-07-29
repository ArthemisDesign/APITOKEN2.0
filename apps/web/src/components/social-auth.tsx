"use client";

import { useEffect, useState } from "react";
import { api, oauthUrl, type ProviderStatus } from "@/lib/api";
import { captureReferralCode, storedReferralCode } from "@/lib/referral";
import { useI18n } from "./i18n-provider";
import { GitHubLogo, GoogleLogo } from "./provider-logos";
import { trackProductEvent } from "@/lib/product-analytics";

export function SocialAuth({ inviteToken }: { inviteToken?: string }) {
  const { t } = useI18n();
  const [providers, setProviders] = useState<ProviderStatus | null>(null);
  // Партнёрский реф-код из localStorage — прокидываем в OAuth-ссылку, чтобы реф партнёра стал
  // B2B до welcome-бонуса. Читаем на клиенте после монтирования.
  const [ref, setRef] = useState<string | undefined>(undefined);
  useEffect(() => { api.providers().then(setProviders).catch(() => setProviders(null)); }, []);
  // Hydrate browser-only referral attribution after mount. Сначала фиксируем ?ref= из ТЕКУЩЕГО URL
  // (last-click wins), затем читаем — чтобы прямой заход на /register?ref=CODE + клик по OAuth не
  // зависел от порядка mount-эффектов и не улетел со старым застрявшим кодом.
  useEffect(() => {
    captureReferralCode(new URLSearchParams(window.location.search).get("ref"));
    const storedRef = storedReferralCode();
    let cancelled = false;
    queueMicrotask(() => {
      if (!cancelled) setRef(storedRef);
    });
    return () => {
      cancelled = true;
    };
  }, []);
  if (!providers) return <div className="social-auth-slot" aria-hidden="true" />;
  if (!providers.google.enabled && !providers.github.enabled) return null;
  return (
    <div className="social-auth-slot social-auth-ready">
      <div className="auth-or"><span>{t("auth_or")}</span></div>
      <div className="social">
        {providers.google.enabled && <a className="btn-social btn-social-google" href={oauthUrl("google", inviteToken, ref)} onClick={() => trackProductEvent("Login Submitted", { method: "google", invited: Boolean(inviteToken), referred: Boolean(ref) })}>
          <GoogleLogo />{t("social_google")}
        </a>}
        {providers.github.enabled && <a className="btn-social btn-social-github" href={oauthUrl("github", inviteToken, ref)} onClick={() => trackProductEvent("Login Submitted", { method: "github", invited: Boolean(inviteToken), referred: Boolean(ref) })}>
          <GitHubLogo />{t("social_github")}
        </a>}
      </div>
    </div>
  );
}
