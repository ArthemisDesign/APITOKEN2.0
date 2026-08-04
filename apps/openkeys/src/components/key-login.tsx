"use client";

import { useState } from "react";
import { AppShell } from "@/components/app-shell";
import { useLanguage } from "@/components/chrome";

const copy = {
  en: {
    titleBar: "Key usage",
    eyebrow: "UNIVERSAL API KEY",
    title: "Open your USAGE dashboard",
    lead: "Paste your sk-pool key to view its live balance and usage. The key is verified by the server and never stored in your browser; only a signed profile session remains.",
    key: "API key",
    missing: "Key not found. Make sure you copied the entire value.",
    unavailable: "Could not reach the server. Please try again.",
    checking: "Checking…",
    submit: "Open USAGE",
    privacy: "Private by design",
    privacyText: "Your secret is used only for verification and is not written to browser storage.",
  },
  ru: {
    titleBar: "Расход ключа",
    eyebrow: "УНИВЕРСАЛЬНЫЙ API-КЛЮЧ",
    title: "Откройте свой USAGE",
    lead: "Вставьте ключ sk-pool, чтобы увидеть его живой баланс и расход. Ключ проверяется сервером и не сохраняется в браузере — остаётся только подписанная сессия профиля.",
    key: "API-ключ",
    missing: "Ключ не найден. Проверьте, что скопировали его целиком.",
    unavailable: "Не удалось связаться с сервером. Попробуйте ещё раз.",
    checking: "Проверяем…",
    submit: "Открыть USAGE",
    privacy: "Приватность по умолчанию",
    privacyText: "Секрет используется только для проверки и не записывается в хранилище браузера.",
  },
} as const;

export function KeyLogin() {
  const { language } = useLanguage();
  const t = copy[language];
  const [key, setKey] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const response = await fetch("/api/usage/lookup", {
        method: "POST",
        cache: "no-store",
        credentials: "same-origin",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ key: key.trim() }),
      });
      if (!response.ok) {
        setError(t.missing);
        return;
      }
      // The session cookie is changed by a Route Handler, outside a Server Action. A document-level
      // navigation guarantees that every browser renders /profile from the new cookie instead of
      // reusing an App Router payload that was created for the previous session.
      window.location.replace("/profile");
    } catch {
      setError(t.unavailable);
    } finally {
      setBusy(false);
    }
  }

  return (
    <AppShell section="profile" title={t.titleBar}>

      <div className="app-body">
        <section className="wrap openkeys-narrow">
          <div className="page-heading">
            <span className="eyebrow">{t.eyebrow}</span>
            <h1 className="p-h1">{t.title}</h1>
            <p className="p-sub">{t.lead}</p>
          </div>

          <form className="card" onSubmit={submit}>
            <div className="field">
              <label htmlFor="apikey">{t.key}</label>
              <input
                id="apikey"
                value={key}
                onChange={(event) => setKey(event.target.value)}
                placeholder="sk-pool-…"
                autoComplete="off"
                spellCheck={false}
              />
            </div>
            {error ? <div className="banner banner-error">{error}</div> : null}
            <button className="btn btn-primary" type="submit" disabled={busy || key.trim() === ""}>
              {busy ? t.checking : t.submit}
            </button>
            <div className="usage-login-privacy"><b>{t.privacy}</b><span>{t.privacyText}</span></div>
          </form>
        </section>
      </div>
    </AppShell>
  );
}
