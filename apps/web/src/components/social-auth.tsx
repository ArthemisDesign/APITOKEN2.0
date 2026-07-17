"use client";

import { useEffect, useState } from "react";
import { api, oauthUrl, type ProviderStatus } from "@/lib/api";
import { useI18n } from "./i18n-provider";
import { GitHubLogo, GoogleLogo } from "./provider-logos";

export function SocialAuth({ inviteToken }: { inviteToken?: string }) {
  const { t } = useI18n();
  const [providers, setProviders] = useState<ProviderStatus | null>(null);
  useEffect(() => { api.providers().then(setProviders).catch(() => setProviders(null)); }, []);
  if (!providers?.google.enabled && !providers?.github.enabled) return null;
  return (
    <>
      <div className="auth-or"><span>{t("auth_or")}</span></div>
      <div className="social">
        {providers.google.enabled && <a className="btn-social btn-social-google" href={oauthUrl("google", inviteToken)}>
          <GoogleLogo />{t("social_google")}
        </a>}
        {providers.github.enabled && <a className="btn-social btn-social-github" href={oauthUrl("github", inviteToken)}>
          <GitHubLogo />{t("social_github")}
        </a>}
      </div>
    </>
  );
}
