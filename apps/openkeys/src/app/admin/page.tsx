"use client";

import { useCallback, useEffect, useState } from "react";

interface IssuedKey {
  secret: string;
  viewToken: string;
  viewUrl: string;
  keyMasked: string;
}

interface BatchRow {
  id: string;
  label: string | null;
  note: string | null;
  quantity: number;
  multBp: number;
  faceValue: string;
  createdAt: string;
}

export default function AdminPage() {
  const [authorized, setAuthorized] = useState(false);
  const [batches, setBatches] = useState<BatchRow[]>([]);
  const [issued, setIssued] = useState<IssuedKey[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    const response = await fetch("/api/admin/batches", { cache: "no-store" });
    if (response.status === 401) {
      setAuthorized(false);
      return;
    }
    const payload = (await response.json()) as { batches: BatchRow[] };
    setAuthorized(true);
    setBatches(payload.batches);
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  async function login(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    const form = new FormData(event.currentTarget);
    try {
      const response = await fetch("/api/admin/login", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ user: form.get("user"), password: form.get("password") }),
      });
      if (!response.ok) {
        setError("Неверный логин или пароль");
        return;
      }
      await refresh();
    } finally {
      setBusy(false);
    }
  }

  async function issue(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    setIssued(null);
    const form = new FormData(event.currentTarget);
    try {
      const response = await fetch("/api/admin/batches", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          faceValueUsd: form.get("faceValueUsd"),
          quantity: Number(form.get("quantity")),
          multBp: form.get("multBp"),
          label: form.get("label"),
          note: form.get("note"),
        }),
      });
      const payload = (await response.json()) as { keys?: IssuedKey[]; error?: string };
      if (!response.ok) {
        setError(payload.error ?? "Не удалось выпустить ключи");
        return;
      }
      setIssued(payload.keys ?? []);
      await refresh();
    } finally {
      setBusy(false);
    }
  }

  async function logout() {
    await fetch("/api/admin/logout", { method: "POST" });
    setAuthorized(false);
    setIssued(null);
  }

  if (!authorized) {
    return (
      <main>
        <h1>Админка OpenKeys</h1>
        <form className="card" onSubmit={login}>
          <label>
            Логин
            <input name="user" autoComplete="username" />
          </label>
          <label>
            Пароль
            <input name="password" type="password" autoComplete="current-password" />
          </label>
          {error ? <p className="error">{error}</p> : null}
          <button type="submit" disabled={busy}>
            {busy ? "Проверяем…" : "Войти"}
          </button>
        </form>
      </main>
    );
  }

  const csv = issued?.map((key) => `${key.secret},${key.viewUrl}`).join("\n") ?? "";

  return (
    <main>
      <div className="topbar" style={{ borderBottom: "none", marginBottom: 8 }}>
        <h1 style={{ margin: 0 }}>Выпуск ключей</h1>
        <button type="button" onClick={logout} style={{ background: "transparent", color: "var(--muted)" }}>
          Выйти
        </button>
      </div>

      <form className="card" onSubmit={issue}>
        <div className="grid2">
          <label>
            Номинал одного ключа, $ (прайс Anthropic)
            <input name="faceValueUsd" defaultValue="50" inputMode="numeric" />
          </label>
          <label>
            Сколько ключей
            <input name="quantity" defaultValue="1" inputMode="numeric" />
          </label>
          <label>
            Множитель, bp (4000 = клиент платит 40%)
            <input name="multBp" placeholder="по умолчанию из env" inputMode="numeric" />
          </label>
          <label>
            Метка партии
            <input name="label" placeholder="funpay-july" />
          </label>
        </div>
        <label>
          Заметка
          <input name="note" placeholder="для кого / где продаётся" />
        </label>
        {error ? <p className="error">{error}</p> : null}
        <button type="submit" disabled={busy}>
          {busy ? "Выпускаем…" : "Выпустить"}
        </button>
      </form>

      {issued ? (
        <div className="card">
          <h2 style={{ marginTop: 0 }}>Выпущено {issued.length} шт.</h2>
          <p className="muted">
            Секреты показываются один раз — мы их не храним. Скопируйте до перезагрузки страницы.
          </p>
          <table>
            <thead>
              <tr>
                <th>Ключ</th>
                <th>Ссылка на расход</th>
              </tr>
            </thead>
            <tbody>
              {issued.map((key) => (
                <tr key={key.viewToken}>
                  <td>
                    <code>{key.secret}</code>
                  </td>
                  <td>
                    <a href={key.viewUrl} target="_blank" rel="noreferrer">
                      {key.viewUrl}
                    </a>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <p style={{ marginTop: 16 }}>
            <button type="button" onClick={() => void navigator.clipboard.writeText(csv)}>
              Скопировать всё в CSV
            </button>
          </p>
        </div>
      ) : null}

      <div className="card">
        <h2 style={{ marginTop: 0 }}>Партии</h2>
        <table>
          <thead>
            <tr>
              <th>Дата</th>
              <th>Метка</th>
              <th>Номинал</th>
              <th>Шт.</th>
              <th>bp</th>
            </tr>
          </thead>
          <tbody>
            {batches.map((batch) => (
              <tr key={batch.id}>
                <td>{batch.createdAt.slice(0, 10)}</td>
                <td>{batch.label ?? "—"}</td>
                <td>{batch.faceValue}</td>
                <td>{batch.quantity}</td>
                <td>{batch.multBp}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </main>
  );
}
