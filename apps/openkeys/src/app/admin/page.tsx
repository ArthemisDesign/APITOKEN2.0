"use client";

import { useCallback, useEffect, useState } from "react";
import { AppShell } from "@/components/app-shell";

const BASE_URL = "https://api.apitoken.sale";

type StockStatus = "stock" | "delivered" | "removed";

interface StockKey {
  id: string;
  status: StockStatus;
  secret: string | null;
  keyMasked: string;
  viewUrl: string;
  faceValue: string;
  label: string | null;
  createdAt: string;
  deliveredAt: string | null;
  removedAt: string | null;
}

const STATUS_LABEL: Record<StockStatus, string> = {
  stock: "на складе",
  delivered: "выдан",
  removed: "снят",
};

/** Готовое сообщение покупателю: адрес, ключ и ссылка на его профиль. */
function handoverText(key: StockKey): string {
  return [
    `ANTHROPIC_BASE_URL=${BASE_URL}`,
    `ANTHROPIC_API_KEY=${key.secret ?? ""}`,
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
  const [keys, setKeys] = useState<StockKey[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    const response = await fetch("/api/admin/keys", { cache: "no-store" });
    if (response.status === 401) {
      setAuthorized(false);
      setChecking(false);
      return;
    }
    const payload = (await response.json()) as { keys: StockKey[] };
    setAuthorized(true);
    setKeys(payload.keys);
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
      const payload = (await response.json()) as { error?: string };
      if (!response.ok) {
        setError(payload.error ?? "Не удалось выпустить ключи");
        return;
      }
      await refresh();
    } finally {
      setBusy(false);
    }
  }

  async function updateKey(id: string, action: "deliver" | "remove") {
    if (action === "remove" && !window.confirm("Снять ключ со склада? Он будет отключён и перестанет работать.")) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const response = await fetch("/api/admin/keys", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ id, action }),
      });
      if (!response.ok) {
        const payload = (await response.json()) as { error?: string };
        setError(payload.error ?? "Не удалось изменить статус ключа");
        return;
      }
      await refresh();
    } finally {
      setBusy(false);
    }
  }

  async function logout() {
    await fetch("/api/admin/logout", { method: "POST" });
    setAuthorized(false);
  }

  if (checking) {
    return (
      <AppShell section="profile" title="Админка">
        <div className="app-body">
          <div className="app-body-in">
            <div className="empty-box">Проверяем сессию…</div>
          </div>
        </div>
      </AppShell>
    );
  }

  if (!authorized) {
    return (
      <AppShell section="profile" title="Админка">
        <div className="app-body">
          <div className="app-body-in">
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
        </div>
      </AppShell>
    );
  }

  const stock = keys.filter((key) => key.status === "stock");
  const history = keys.filter((key) => key.status !== "stock");
  const allStockText = stock.map((key) => handoverText(key)).join("\n\n———\n\n");

  return (
    <AppShell
      section="profile"
      title="Выпуск ключей"
      actions={
        <button className="btn btn-ghost btn-sm" type="button" onClick={logout}>
          Выйти
        </button>
      }
    >
      <div className="app-body">
        <div className="app-body-in">
          <form className="card" onSubmit={issue}>
            <div className="openkeys-form-grid">
              <div className="field">
                <label htmlFor="faceValueUsd">Номинал ключа, $</label>
                <input id="faceValueUsd" name="faceValueUsd" defaultValue="50" inputMode="numeric" />
                <span className="field-hint">баланс Claude API</span>
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

          <section className="dsec">
            <div className="dsec-head analytics-heading">
              <div>
                <h2>Склад · {stock.length}</h2>
                <p>Ключи, готовые к продаже. После отметки «выдан» ключ уходит в историю.</p>
              </div>
              {stock.length > 0 ? (
                <div className="overview-card-actions">
                  <CopyButton value={allStockText} label="Скопировать весь склад" />
                </div>
              ) : null}
            </div>

            {stock.length === 0 ? (
              <div className="empty-box">Склад пуст — выпустите ключи формой выше</div>
            ) : (
              stock.map((key) => (
                <article className="card openkeys-issued" key={key.id}>
                  <div className="openkeys-issued-head">
                    <span className="pill">{key.faceValue}</span>
                    {key.label ? <span className="chip">{key.label}</span> : null}
                    <span className="muted-note">{key.createdAt.slice(0, 10)}</span>
                  </div>
                  <div className="secret-key-field">
                    <code>{key.secret ?? key.keyMasked}</code>
                    {key.secret ? <CopyButton value={key.secret} label="Ключ" /> : null}
                  </div>
                  <div className="secret-key-field">
                    <code>{BASE_URL}</code>
                    <CopyButton value={BASE_URL} label="Base URL" />
                  </div>
                  <div className="secret-key-field">
                    <code>{key.viewUrl}</code>
                    <CopyButton value={key.viewUrl} label="Профиль" />
                  </div>
                  <div className="openkeys-issued-foot">
                    <CopyButton value={handoverText(key)} label="Сообщение покупателю" primary />
                    <button
                      className="btn btn-ghost btn-sm"
                      type="button"
                      disabled={busy}
                      onClick={() => void updateKey(key.id, "deliver")}
                    >
                      Выдан
                    </button>
                    <button
                      className="btn btn-ghost btn-sm openkeys-danger"
                      type="button"
                      disabled={busy}
                      onClick={() => void updateKey(key.id, "remove")}
                    >
                      Удалить
                    </button>
                  </div>
                </article>
              ))
            )}
          </section>

          <section className="dsec">
            <div className="dsec-head analytics-heading">
              <div>
                <h2>История</h2>
                <p>Выданные и снятые ключи. Секрет здесь уже недоступен — остаётся маска и профиль.</p>
              </div>
            </div>
            <div className="table-scroll" role="region" tabIndex={0} aria-label="История ключей">
              <table className="mtable">
                <thead>
                  <tr>
                    <th>Ключ</th>
                    <th>Статус</th>
                    <th>Метка</th>
                    <th className="tnum">Номинал</th>
                    <th className="tnum">Выпущен</th>
                    <th className="tnum">Событие</th>
                    <th>Профиль</th>
                  </tr>
                </thead>
                <tbody>
                  {history.length === 0 ? (
                    <tr>
                      <td colSpan={7} className="empty-cell">
                        Пока ничего не выдано и не снято
                      </td>
                    </tr>
                  ) : (
                    history.map((key) => (
                      <tr key={key.id}>
                        <td>
                          <code className="key-mask">{key.keyMasked}</code>
                        </td>
                        <td>
                          <span className={`pill ${key.status === "removed" ? "pill-muted" : "pill-good"}`}>
                            {STATUS_LABEL[key.status]}
                          </span>
                        </td>
                        <td>{key.label ?? "—"}</td>
                        <td className="tnum">{key.faceValue}</td>
                        <td className="tnum">{key.createdAt.slice(0, 10)}</td>
                        <td className="tnum">{(key.deliveredAt ?? key.removedAt ?? "").slice(0, 10) || "—"}</td>
                        <td>
                          <a href={key.viewUrl} target="_blank" rel="noreferrer">
                            открыть
                          </a>
                        </td>
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
