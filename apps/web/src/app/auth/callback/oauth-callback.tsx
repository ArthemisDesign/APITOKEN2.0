"use client";

import { useRouter, useSearchParams } from "next/navigation";
import { useEffect, useState } from "react";
import { api } from "@/lib/api";
import { AuthIntro, Feedback } from "@/components/auth-shell";
import { LocalizedLink } from "@/components/translated";
import { trackProductEvent, trackSuccessfulLogin } from "@/lib/product-analytics";
import { localeHref } from "@/lib/locale-routes";
import { browserStorage, readSavedLanguage } from "@/lib/user-preferences";

export type OAuthCallbackPhase = "loading" | "error";

export function oauthFeedbackIsSuccess(phase: OAuthCallbackPhase): boolean {
  return phase !== "error";
}

export function OAuthCallback() {
  const search = useSearchParams();
  const router = useRouter();
  const error = search.get("error");
  const provider = search.get("provider") === "github" ? "github" : "google";
  const [message, setMessage] = useState(error ? `Sign-in failed: ${error.replaceAll("_", " ")}` : "Completing sign-in…");
  const [phase, setPhase] = useState<OAuthCallbackPhase>(error ? "error" : "loading");
  useEffect(() => {
    if (error) { trackProductEvent("Login Failed", { method: provider, status: "oauth_callback" }); return; }
    api.me().then(() => {
      trackSuccessfulLogin(provider);
      router.replace(localeHref("/dashboard", readSavedLanguage(browserStorage()) ?? "en"));
    })
      .catch(() => {
        trackProductEvent("Login Failed", { method: provider, status: "session_confirmation" });
        setPhase("error");
        setMessage("The sign-in session could not be confirmed.");
      });
  }, [error, provider, router]);
  return <><AuthIntro title="Social sign-in" subtitle="Returning securely to apiToken.sale." />
    <Feedback message={message} success={oauthFeedbackIsSuccess(phase)} /><div className="auth-alt"><LocalizedLink href="/login">Back to login</LocalizedLink></div></>;
}
