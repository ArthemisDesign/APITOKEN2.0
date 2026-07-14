"use client";

import { useEffect, useState } from "react";
import { api, oauthUrl, type ProviderStatus } from "@/lib/api";
import { useI18n } from "./i18n-provider";

export function SocialAuth({ inviteToken }: { inviteToken?: string }) {
  const { t } = useI18n();
  const [providers, setProviders] = useState<ProviderStatus | null>(null);
  useEffect(() => { api.providers().then(setProviders).catch(() => setProviders(null)); }, []);
  if (!providers?.google.enabled && !providers?.github.enabled) return null;
  return (
    <>
      <div className="auth-or"><span>{t("auth_or")}</span></div>
      <div className="social">
        {providers.google.enabled && <a className="btn-social" href={oauthUrl("google", inviteToken)}>
          <span aria-hidden="true">G</span>{t("social_google")}
        </a>}
        {providers.github.enabled && <a className="btn-social" href={oauthUrl("github", inviteToken)}>
          <span aria-hidden="true">GH</span>{t("social_github")}
        </a>}
      </div>
    </>
  );
}
