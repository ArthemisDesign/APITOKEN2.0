"use client";

import Link from "next/link";
import { useCallback, useEffect, useState } from "react";
import { AppShell } from "@/components/app-shell";

const BASE_URL = "https://api.apitoken.sale";
const DOCS_URL = "https://openkeys.apitoken.sale/docs";

type StockStatus = "stock" | "delivered";

interface StockKey {
  id: string;
  batchId: string;
  status: StockStatus;
  secret: string | null;
  keyMasked: string;
  viewUrl: string;
  faceValue: string;
  faceValueNano: string;
  label: string | null;
  createdAt: string;
  deliveredAt: string | null;
}

interface BatchRow {
  id: string;
  label: string | null;
  faceValue: string;
  quantity: number;
  createdAt: string;
}

const STATUS_LABEL: Record<StockStatus, string> = {
  stock: "на складе",
  delivered: "выдан",
};

/** Готовое сообщение покупателю: баланс, адрес, ключ, профиль и инструкция. */
function handoverText(key: StockKey): string {
  return [
    `Баланс ключа: ${key.faceValue} по прайсу Anthropic`,
    "",
    `ANTHROPIC_BASE_URL=${BASE_URL}`,
    `ANTHROPIC_API_KEY=${key.secret ?? ""}`,
    "",
    `Остаток и расход: ${key.viewUrl}`,
    `Как подключить: ${DOCS_URL}`,
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

function KeyCard({
  keyRow,
  busy,
  onUpdate,
}: {
  keyRow: StockKey;
  busy: boolean;
  onUpdate(id: string, action: "deliver" | "remove"): void;
}) {
  return (
    <article className="card openkeys-issued">
      <div className="openkeys-issued-head">
        <span className="pill">{keyRow.faceValue}</span>
        {keyRow.label ? <span className="chip">{keyRow.label}</span> : null}
        <span className="field-hint">{keyRow.createdAt.slice(0, 10)}</span>
      </div>
      <div className="secret-key-field">
        <code>{keyRow.secret ?? keyRow.keyMasked}</code>
        {keyRow.secret ? <CopyButton value={keyRow.secret} label="Ключ" /> : null}
      </div>
      <div className="secret-key-field">
        <code>{BASE_URL}</code>
        <CopyButton value={BASE_URL} label="Base URL" />
      </div>
      <div className="secret-key-field">
        <code>{keyRow.viewUrl}</code>
        <CopyButton value={keyRow.viewUrl} label="Профиль" />
      </div>
      <div className="secret-key-field">
        <code>{DOCS_URL}</code>
        <CopyButton value={DOCS_URL} label="Документация" />
      </div>
      <div className="openkeys-issued-foot">
        <CopyButton value={handoverText(keyRow)} label="Сообщение покупателю" primary />
        <button
          className="btn btn-ghost btn-sm"
          type="button"
          disabled={busy}
          onClick={() => onUpdate(keyRow.id, "deliver")}
        >
          Выдан
        </button>
        <button
          className="btn btn-ghost btn-sm openkeys-danger"
          type="button"
          disabled={busy}
          onClick={() => onUpdate(keyRow.id, "remove")}
        >
          Удалить
        </button>
      </div>
    </article>
  );
}

export default function AdminPage() {
  const [authorized, setAuthorized] = useState(false);
  const [checking, setChecking] = useState(true);
  const [admin, setAdmin] = useState<string | null>(null);
  const [keys, setKeys] = useState<StockKey[]>([]);
  const [batches, setBatches] = useState<BatchRow[]>([]);
  const [openBatch, setOpenBatch] = useState<string | null>(null);
  const [showIssueForm, setShowIssueForm] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const response = await fetch("/api/admin/keys", { cache: "no-store" });
      if (response.status === 401) {
        setAuthorized(false);
        return;
      }
      if (!response.ok) throw new Error(`admin data failed: ${response.status}`);

      const payload = (await response.json()) as { admin: string; keys: StockKey[]; batches: BatchRow[] };
      setAuthorized(true);
      setAdmin(payload.admin);
      setKeys(payload.keys);
      setBatches(payload.batches);
      setError(null);
    } catch {
      setError("Не удалось загрузить ключи. Повторите попытку.");
    }
  }, []);

  useEffect(() => {
    void (async () => {
      try {
        const response = await fetch("/api/admin/session", { cache: "no-store" });
        if (response.status === 401) {
          setAuthorized(false);
          return;
        }
        if (!response.ok) throw new Error(`session check failed: ${response.status}`);
        const payload = (await response.json()) as { admin: string };
        setAuthorized(true);
        setAdmin(payload.admin);
        await refresh();
      } catch {
        setAuthorized(false);
        setError("Не удалось проверить сессию. Обновите страницу и попробуйте снова.");
      } finally {
        setChecking(false);
      }
    })();
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
      const payload = (await response.json()) as { admin: string };
      setAuthorized(true);
      setAdmin(payload.admin);
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
      setShowIssueForm(false);
      await refresh();
    } finally {
      setBusy(false);
    }
  }

  async function updateKey(id: string, action: "deliver" | "remove") {
    if (action === "remove" && !window.confirm("Удалить ключ? Он будет отключён и исчезнет из системы.")) {
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

  async function removeAll(count: number) {
    if (!window.confirm(`Удалить все ${count} ключей со склада? Они будут отключены и исчезнут из системы.`)) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const response = await fetch("/api/admin/keys", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ action: "remove_all" }),
      });
      if (!response.ok) {
        setError("Не удалось очистить склад");
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
      <AppShell section="stock" title="Админка">
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
      <AppShell section="stock" title="Админка">
        <div className="app-body">
          <div className="app-body-in">
            <section className="wrap openkeys-narrow">
              <div className="page-heading">
                <span className="eyebrow">apiToken</span>
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

  // Склад группируем по номиналу: продавать удобнее пачками одного достоинства.
  const groups = new Map<string, StockKey[]>();
  for (const key of stock) {
    const bucket = groups.get(key.faceValueNano);
    if (bucket) bucket.push(key);
    else groups.set(key.faceValueNano, [key]);
  }
  const groupList = [...groups.entries()].sort(([left], [right]) => {
    const difference = BigInt(right) - BigInt(left);
    return difference > 0n ? 1 : difference < 0n ? -1 : 0;
  });

  return (
    <AppShell
      section="stock"
      title="Выпуск ключей"
      actions={
        <>
          <button className="btn btn-primary btn-sm" type="button" onClick={() => setShowIssueForm((open) => !open)}>
            {showIssueForm ? "Закрыть" : "Выпустить пачку"}
          </button>
          <button className="btn btn-ghost btn-sm" type="button" onClick={logout}>
            Выйти
          </button>
        </>
      }
    >
      <div className="app-body">
        <div className="app-body-in">
          {admin ? <p className="field-hint">Показаны только ключи, выпущенные под учёткой {admin}.</p> : null}
          {error ? <div className="banner banner-error">{error}</div> : null}

          {showIssueForm ? (
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
              <button className="btn btn-primary" type="submit" disabled={busy}>
                {busy ? "Выпускаем…" : "Выпустить"}
              </button>
            </form>
          ) : null}

          <section className="dsec">
            <div className="dsec-head analytics-heading">
              <div>
                <h2>Склад · {stock.length}</h2>
                <p>Ключи, готовые к продаже. «Выдан» прячет ключ в историю, «Удалить» стирает его из системы.</p>
              </div>
              {stock.length > 0 ? (
                <button
                  className="btn btn-ghost btn-sm openkeys-danger"
                  type="button"
                  disabled={busy}
                  onClick={() => void removeAll(stock.length)}
                >
                  Удалить всё
                </button>
              ) : null}
            </div>

            {stock.some((key) => !key.secret) ? (
              <div className="banner">
                Часть ключей выпущена до появления склада, их секрет не сохранился — выдать такой ключ уже нельзя,
                его можно только удалить.
              </div>
            ) : null}

            {stock.length === 0 ? (
              <div className="empty-box">Склад пуст — выпустите пачку кнопкой сверху</div>
            ) : (
              groupList.map(([faceValueNano, groupKeys]) => (
                <div key={faceValueNano} className="openkeys-group">
                  <div className="openkeys-group-head">
                    <h3>
                      {groupKeys[0]!.faceValue} · {groupKeys.length} шт.
                    </h3>
                    {/* В групповое копирование берём только ключи с секретом: строка
                        без ключа в сообщении покупателю хуже, чем её отсутствие. */}
                    {groupKeys.some((key) => key.secret) ? (
                      <CopyButton
                        value={groupKeys
                          .filter((key) => key.secret)
                          .map((key) => handoverText(key))
                          .join("\n\n———\n\n")}
                        label={`Скопировать ${groupKeys.filter((key) => key.secret).length} шт.`}
                      />
                    ) : null}
                  </div>
                  {groupKeys.map((key) => (
                    <KeyCard key={key.id} keyRow={key} busy={busy} onUpdate={updateKey} />
                  ))}
                </div>
              ))
            )}
          </section>

          <section className="dsec">
            <div className="dsec-head analytics-heading">
              <div>
                <h2>Партии</h2>
                <p>Откройте партию, чтобы увидеть выпущенные в ней ключи.</p>
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
                    <th className="tnum">На складе</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {batches.length === 0 ? (
                    <tr>
                      <td colSpan={6} className="empty-cell">
                        Пока ничего не выпускали
                      </td>
                    </tr>
                  ) : (
                    batches.map((batch) => {
                      const inStock = keys.filter(
                        (key) => key.batchId === batch.id && key.status === "stock",
                      ).length;
                      return (
                        <tr key={batch.id}>
                          <td>{batch.createdAt.slice(0, 10)}</td>
                          <td>{batch.label ?? "—"}</td>
                          <td className="tnum">{batch.faceValue}</td>
                          <td className="tnum">{batch.quantity}</td>
                          <td className="tnum">{inStock}</td>
                          <td>
                            <button
                              className="btn btn-ghost btn-sm"
                              type="button"
                              onClick={() => setOpenBatch((current) => (current === batch.id ? null : batch.id))}
                            >
                              {openBatch === batch.id ? "Скрыть" : "Открыть"}
                            </button>
                          </td>
                        </tr>
                      );
                    })
                  )}
                </tbody>
              </table>
            </div>

            {openBatch ? (
              <div className="openkeys-group">
                <div className="openkeys-group-head">
                  <h3>Ключи партии</h3>
                </div>
                <div className="table-scroll" role="region" tabIndex={0} aria-label="Ключи партии">
                  <table className="mtable">
                    <thead>
                      <tr>
                        <th>Ключ</th>
                        <th>Статус</th>
                        <th className="tnum">Номинал</th>
                        <th>Профиль</th>
                      </tr>
                    </thead>
                    <tbody>
                      {keys
                        .filter((key) => key.batchId === openBatch)
                        .map((key) => (
                          <tr key={key.id}>
                            <td>
                              <code className="key-mask">{key.keyMasked}</code>
                            </td>
                            <td>{STATUS_LABEL[key.status]}</td>
                            <td className="tnum">{key.faceValue}</td>
                            <td>
                              <a href={key.viewUrl} target="_blank" rel="noreferrer">
                                открыть
                              </a>
                            </td>
                          </tr>
                        ))}
                    </tbody>
                  </table>
                </div>
              </div>
            ) : null}
          </section>

          <section className="dsec">
            <div className="dsec-head analytics-heading">
              <div>
                <h2>История</h2>
                <p>Выданные ключи. Секрет здесь уже недоступен — остаётся маска и профиль.</p>
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
                          <span className="pill pill-good">
                            {STATUS_LABEL[key.status]}
                          </span>
                        </td>
                        <td>{key.label ?? "—"}</td>
                        <td className="tnum">{key.faceValue}</td>
                        <td className="tnum">{key.createdAt.slice(0, 10)}</td>
                        <td className="tnum">{(key.deliveredAt ?? "").slice(0, 10) || "—"}</td>
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
