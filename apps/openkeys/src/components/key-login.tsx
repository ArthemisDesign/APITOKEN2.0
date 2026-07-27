"use client";

import { useRouter } from "next/navigation";
import { useState } from "react";
import { AppShell } from "@/components/app-shell";

export function KeyLogin() {
  const router = useRouter();
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
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ key: key.trim() }),
      });
      if (!response.ok) {
        setError("Ключ не найден. Проверьте, что скопировали его целиком.");
        return;
      }
      router.refresh();
    } catch {
      setError("Не удалось связаться с сервером. Попробуйте ещё раз.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <AppShell section="profile" title="Вход по ключу">

      <div className="app-body">
        <section className="wrap openkeys-narrow">
          <div className="page-heading">
            <span className="eyebrow">Профиль ключа</span>
            <h1 className="p-h1">Вход по ключу</h1>
            <p className="p-sub">
              Вставьте свой ключ — откроется его профиль с остатком и расходом. Ключ проверяется у сервера и не
              сохраняется в браузере: в куке остаётся только ссылка на этот профиль.
            </p>
          </div>

          <form className="card" onSubmit={submit}>
            <div className="field">
              <label htmlFor="apikey">API-ключ</label>
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
              {busy ? "Проверяем…" : "Войти"}
            </button>
          </form>
        </section>
      </div>
    </AppShell>
  );
}
