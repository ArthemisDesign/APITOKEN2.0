"use client";

import { useCallback, useEffect, useState } from "react";
import { AppShell } from "@/components/app-shell";

const BASE_URL = "https://api.apitoken.sale";

interface IssuedKey {
  secret: string;
  viewToken: string;
  viewUrl: string;
  keyMasked: string;
}

interface BatchRow {
  id: string;
  label: string | null;
  quantity: number;
  multBp: number;
  faceValue: string;
  createdAt: string;
}

/** Готовое сообщение покупателю: адрес, ключ и ссылка на его расход. */
function handoverText(key: IssuedKey): string {
  return [
    `ANTHROPIC_BASE_URL=${BASE_URL}`,
    `ANTHROPIC_API_KEY=${key.secret}`,
    "",
    `Остаток и расход: ${key.viewUrl}`,
    "Как подключить: https://openkeys.apitoken.sale/docs",
  ].join("\n");
}

function CopyButton({ value, label, primary = false }: { value: string; label: string; primary?: boolean }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      className={`btn btn-sm ${primary ? "btn-primary" : "btn-ghost"}`}
      onClick={() => {
        void navigator.clipboard.writeText(value).then(() => {
          setCopied(true);
          setTimeout(() => setCopied(false), 1600);
        });
      }}
    >
      {copied ? "Скопировано" : label}
    </button>
  );
}

export default function AdminPage() {
  const [authorized, setAuthorized] = useState(false);
  const [checking, setChecking] = useState(true);
  const [batches, setBatches] = useState<BatchRow[]>([]);
  const [issued, setIssued] = useState<IssuedKey[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    const response = await fetch("/api/admin/batches", { cache: "no-store" });
    if (response.status === 401) {
      setAuthorized(false);
      setChecking(false);
      return;
    }
    const payload = (await response.json()) as { batches: BatchRow[] };
    setAuthorized(true);
    setBatches(payload.batches);
    setChecking(false);
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
          label: form.get("label"),
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

  if (checking) {
    return (
      <AppShell section="profile" title="Админка">

        <div className="app-body">
          <section className="wrap openkeys-narrow">
            <div className="empty-box">Проверяем сессию…</div>
          </section>
        </div>
      </AppShell>
    );
  }

  if (!authorized) {
    return (
      <AppShell section="profile" title="Админка">

        <div className="app-body">
          <section className="wrap openkeys-narrow">
            <div className="page-heading">
              <span className="eyebrow">OpenKeys</span>
              <h1 className="p-h1">Вход в админку</h1>
            </div>
            <form className="card" onSubmit={login}>
              <div className="field">
                <label htmlFor="user">Логин</label>
                <input id="user" name="user" autoComplete="username" />
              </div>
              <div className="field">
                <label htmlFor="password">Пароль</label>
                <input id="password" name="password" type="password" autoComplete="current-password" />
              </div>
              {error ? <div className="banner banner-error">{error}</div> : null}
              <button className="btn btn-primary" type="submit" disabled={busy}>
                {busy ? "Проверяем…" : "Войти"}
              </button>
            </form>
          </section>
        </div>
      </AppShell>
    );
  }

  const allKeysText = issued?.map((key) => handoverText(key)).join("\n\n———\n\n") ?? "";
  const csv = issued?.map((key) => `${key.secret},${key.viewUrl}`).join("\n") ?? "";

  return (
    <AppShell section="profile" title="Выпуск ключей">

      <div className="app-body">
        <div className="app-body-in">
          <div className="dsec-head analytics-heading">
            <div className="page-heading openkeys-heading-flush">
              <span className="eyebrow">OpenKeys</span>
              <h1 className="p-h1">Выпуск ключей</h1>
              <p className="p-sub">Номинал задаётся в долларах официального прайса Anthropic.</p>
            </div>
            <button className="btn btn-ghost btn-sm" type="button" onClick={logout}>
              Выйти
            </button>
          </div>

          <form className="card" onSubmit={issue}>
            <div className="openkeys-form-grid">
              <div className="field">
                <label htmlFor="faceValueUsd">Номинал ключа, $</label>
                <input id="faceValueUsd" name="faceValueUsd" defaultValue="50" inputMode="numeric" />
                <span className="field-hint">по прайсу Anthropic</span>
              </div>
              <div className="field">
                <label htmlFor="quantity">Количество</label>
                <input id="quantity" name="quantity" defaultValue="1" inputMode="numeric" />
                <span className="field-hint">до 100 за раз</span>
              </div>
              <div className="field">
                <label htmlFor="label">Метка партии</label>
                <input id="label" name="label" placeholder="funpay-july" />
                <span className="field-hint">чтобы отличать поставки</span>
              </div>
            </div>
            {error ? <div className="banner banner-error">{error}</div> : null}
            <button className="btn btn-primary" type="submit" disabled={busy}>
              {busy ? "Выпускаем…" : "Выпустить"}
            </button>
          </form>

          {issued ? (
            <section className="dsec">
              <div className="dsec-head analytics-heading">
                <div>
                  <h2>Выпущено {issued.length} шт.</h2>
                  <p>Секреты показываются один раз — мы их не храним. Скопируйте до перезагрузки страницы.</p>
                </div>
                <div className="overview-card-actions">
                  <CopyButton value={allKeysText} label="Всё для покупателя" primary />
                  <CopyButton value={csv} label="CSV" />
                </div>
              </div>

              {issued.map((key) => (
                <article className="card openkeys-issued" key={key.viewToken}>
                  <div className="secret-key-field">
                    <code>{key.secret}</code>
                    <CopyButton value={key.secret} label="Ключ" />
                  </div>
                  <div className="secret-key-field">
                    <code>{BASE_URL}</code>
                    <CopyButton value={BASE_URL} label="Base URL" />
                  </div>
                  <div className="secret-key-field">
                    <code>{key.viewUrl}</code>
                    <CopyButton value={key.viewUrl} label="Ссылка на расход" />
                  </div>
                  <div className="openkeys-issued-foot">
                    <CopyButton value={handoverText(key)} label="Сообщение покупателю" primary />
                    <a className="btn btn-ghost btn-sm" href={key.viewUrl} target="_blank" rel="noreferrer">
                      Открыть расход
                    </a>
                  </div>
                </article>
              ))}
            </section>
          ) : null}

          <section className="dsec">
            <div className="dsec-head analytics-heading">
              <div>
                <h2>Партии</h2>
                <p>История выпусков.</p>
              </div>
            </div>
            <div className="table-scroll" role="region" tabIndex={0} aria-label="Партии">
              <table className="mtable">
                <thead>
                  <tr>
                    <th>Дата</th>
                    <th>Метка</th>
                    <th className="tnum">Номинал</th>
                    <th className="tnum">Шт.</th>
                  </tr>
                </thead>
                <tbody>
                  {batches.length === 0 ? (
                    <tr>
                      <td colSpan={4} className="empty-cell">
                        Пока ничего не выпускали
                      </td>
                    </tr>
                  ) : (
                    batches.map((batch) => (
                      <tr key={batch.id}>
                        <td>{batch.createdAt.slice(0, 10)}</td>
                        <td>{batch.label ?? "—"}</td>
                        <td className="tnum">{batch.faceValue}</td>
                        <td className="tnum">{batch.quantity}</td>
                      </tr>
                    ))
                  )}
                </tbody>
              </table>
            </div>
          </section>
        </div>
      </div>
    </AppShell>
  );
}
