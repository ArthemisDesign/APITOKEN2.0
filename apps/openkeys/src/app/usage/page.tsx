"use client";

import { useRouter } from "next/navigation";
import { useState } from "react";
import { SiteHeader } from "@/components/chrome";

export default function UsageLookupPage() {
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
      const payload = (await response.json()) as { viewToken: string };
      router.push(`/u/${payload.viewToken}`);
    } catch {
      setError("Не удалось связаться с сервером. Попробуйте ещё раз.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <SiteHeader />
      <main id="main-content">
        <section className="wrap openkeys-narrow">
          <div className="page-heading">
            <span className="eyebrow">Мой расход</span>
            <h1 className="p-h1">Остаток по ключу</h1>
            <p className="p-sub">
              Вставьте ключ — откроется его страница расхода. Ключ нигде не сохраняется и нужен только для того, чтобы
              найти нужный баланс.
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
              {busy ? "Проверяем…" : "Показать расход"}
            </button>
          </form>
        </section>
      </main>
    </>
  );
}
