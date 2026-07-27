"use client";

import { useRouter } from "next/navigation";
import { useState } from "react";

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
    <main>
      <h1>Мой расход</h1>
      <p className="muted">
        Вставьте свой ключ — покажем остаток и потраченное. Ключ никуда не сохраняется и нужен только для того,
        чтобы найти нужный баланс.
      </p>

      <form className="card" onSubmit={submit}>
        <label>
          API-ключ
          <input
            value={key}
            onChange={(event) => setKey(event.target.value)}
            placeholder="sk-pool-…"
            autoComplete="off"
            spellCheck={false}
          />
        </label>
        {error ? <p className="error">{error}</p> : null}
        <button type="submit" disabled={busy || key.trim() === ""}>
          {busy ? "Проверяем…" : "Показать баланс"}
        </button>
      </form>
    </main>
  );
}
