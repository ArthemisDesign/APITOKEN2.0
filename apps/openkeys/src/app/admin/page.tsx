"use client";

import { Fragment, useCallback, useEffect, useRef, useState } from "react";
import { AppShell } from "@/components/app-shell";
import {
  OPENKEYS_PUBLIC_ORIGIN,
  UNIVERSAL_CONNECTIONS,
  universalKeyHandoverText,
} from "@/lib/universal-key";

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
  pricingContract: "legacy" | "official_1_to_1";
  label: string | null;
  enabled: boolean;
  createdAt: string;
  deliveredAt: string | null;
}

interface BatchRow {
  id: string;
  label: string | null;
  faceValue: string;
  pricingContract: "legacy" | "official_1_to_1";
  quantity: number;
  stockCount: number;
  deliveredCount: number;
  disabledCount: number;
  createdAt: string;
}

interface BatchPayload {
  admin: string;
  batches: BatchRow[];
  total: number;
  limit: number;
  offset: number;
  totals: { stock: number; delivered: number; disabled: number };
  issuanceAuthority: { ready: boolean; supportedModels: string[] };
}

function CopyButton({
  value,
  label,
  primary = false,
  disabled = false,
}: {
  value: string;
  label: string;
  primary?: boolean;
  disabled?: boolean;
}) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      className={`btn btn-sm ${primary ? "btn-primary" : "btn-ghost"}`}
      disabled={disabled}
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
  const canHandover = keyRow.enabled && keyRow.secret !== null;
  return (
    <article className={`card openkeys-key-card ${keyRow.enabled ? "" : "is-disabled"}`}>
      <div className="openkeys-issued-head">
        <span className={`pill ${keyRow.enabled ? "pill-good" : "pill-muted"}`}>
          {keyRow.enabled ? "готов к продаже" : "отключён"}
        </span>
        <span className="pill">{keyRow.faceValue}</span>
        <span className={`pill ${keyRow.pricingContract === "official_1_to_1" ? "pill-good" : "pill-muted"}`}>
          {keyRow.pricingContract === "official_1_to_1" ? "1:1 official" : "legacy"}
        </span>
        <span className="chip">{keyRow.label ?? "Без метки"}</span>
      </div>
      <div className="secret-key-field openkeys-key-secret">
        <code>{keyRow.secret ?? keyRow.keyMasked}</code>
        {keyRow.secret ? <CopyButton value={keyRow.secret} label="Ключ" disabled={!keyRow.enabled} /> : null}
      </div>
      <div className="openkeys-key-meta">
        <span>создан {keyRow.createdAt.slice(0, 10)}</span>
        <a href={keyRow.viewUrl} target="_blank" rel="noreferrer">Профиль ↗</a>
      </div>
      {!keyRow.secret ? (
        <p className="field-hint">Секрет этого старого ключа не сохранился — продавать его нельзя.</p>
      ) : null}
      <div className="openkeys-issued-foot">
        <CopyButton
          value={universalKeyHandoverText(keyRow)}
          label="Сообщение покупателю"
          primary
          disabled={!canHandover}
        />
        <button
          className="btn btn-ghost btn-sm"
          type="button"
          disabled={busy || !canHandover}
          onClick={() => onUpdate(keyRow.id, "deliver")}
        >
          Отметить выданным
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
  const [batchTotal, setBatchTotal] = useState(0);
  const [batchLimit, setBatchLimit] = useState(20);
  const [totals, setTotals] = useState({ stock: 0, delivered: 0, disabled: 0 });
  const [openBatch, setOpenBatch] = useState<string | null>(null);
  const [batchOffset, setBatchOffset] = useState(0);
  const [batchQuery, setBatchQuery] = useState("");
  const [batchQueryDraft, setBatchQueryDraft] = useState("");
  const [showIssueForm, setShowIssueForm] = useState(false);
  const [issuanceAuthority, setIssuanceAuthority] = useState({
    ready: false,
    supportedModels: [] as string[],
  });
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const openBatchRef = useRef<string | null>(null);
  const batchOffsetRef = useRef(0);
  const batchQueryRef = useRef("");

  const chooseBatch = useCallback((batchId: string | null) => {
    openBatchRef.current = batchId;
    setOpenBatch(batchId);
    if (batchId === null) setKeys([]);
  }, []);

  const refresh = useCallback(async (options: {
    batchId?: string | null;
    offset?: number;
    query?: string;
    openLatest?: boolean;
  } = {}) => {
    const offset = options.offset ?? batchOffsetRef.current;
    const query = options.query ?? batchQueryRef.current;
    try {
      const params = new URLSearchParams({ limit: "20", offset: String(offset) });
      if (query) params.set("q", query);
      const batchResponse = await fetch(`/api/admin/batches?${params}`, { cache: "no-store" });
      if (batchResponse.status === 401) {
        setAuthorized(false);
        return;
      }
      if (!batchResponse.ok) throw new Error(`batch data failed: ${batchResponse.status}`);
      const payload = (await batchResponse.json()) as BatchPayload;
      const selected = options.batchId !== undefined
        ? options.batchId
        : options.openLatest && openBatchRef.current === null
          ? payload.batches[0]?.id ?? null
          : openBatchRef.current;
      let nextKeys: StockKey[] = [];
      if (selected) {
        const keyResponse = await fetch(`/api/admin/keys?batchId=${encodeURIComponent(selected)}`, { cache: "no-store" });
        if (keyResponse.status === 401) {
          setAuthorized(false);
          return;
        }
        if (!keyResponse.ok) throw new Error(`key data failed: ${keyResponse.status}`);
        nextKeys = ((await keyResponse.json()) as { keys: StockKey[] }).keys;
      }

      batchOffsetRef.current = payload.offset;
      batchQueryRef.current = query;
      setBatchOffset(payload.offset);
      setBatchQuery(query);
      setBatchLimit(payload.limit);
      setBatchTotal(payload.total);
      setBatches(payload.batches);
      setTotals(payload.totals);
      setIssuanceAuthority(payload.issuanceAuthority);
      setKeys(nextKeys);
      chooseBatch(selected);
      setAuthorized(true);
      setAdmin(payload.admin);
      setError(null);
    } catch {
      setError("Не удалось загрузить партии и ключи. Повторите попытку.");
    }
  }, [chooseBatch]);

  useEffect(() => {
    void (async () => {
      try {
        const response = await fetch("/api/admin/session", { cache: "no-store" });
        if (response.status === 401) {
          setAuthorized(false);
          return;
        }
        if (!response.ok) throw new Error(`session check failed: ${response.status}`);
        setAuthorized(true);
        await refresh({ openLatest: true });
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
      setAuthorized(true);
      await refresh({ offset: 0, query: "", openLatest: true });
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
      const payload = (await response.json()) as { batchId?: string; error?: string };
      if (!response.ok || !payload.batchId) {
        setError(payload.error ?? "Не удалось выпустить ключи");
        return;
      }
      setShowIssueForm(false);
      setBatchQueryDraft("");
      await refresh({ batchId: payload.batchId, offset: 0, query: "" });
    } finally {
      setBusy(false);
    }
  }

  async function updateKey(id: string, action: "deliver" | "remove") {
    if (action === "remove" && !window.confirm("Удалить ключ? Он будет отключён и снят со склада.")) return;
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

  async function removeBatchStock(count: number) {
    if (!openBatch || !window.confirm(`Удалить ${count} ключей со склада выбранной партии? Они будут отключены.`)) return;
    setBusy(true);
    setError(null);
    try {
      const response = await fetch("/api/admin/keys", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ action: "remove_all", batchId: openBatch }),
      });
      if (!response.ok) {
        setError("Не удалось очистить остаток партии");
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
    return <AppShell section="stock" title="Админка"><div className="app-body"><div className="app-body-in"><div className="empty-box">Проверяем сессию…</div></div></div></AppShell>;
  }

  if (!authorized) {
    return (
      <AppShell section="stock" title="Админка">
        <div className="app-body"><div className="app-body-in">
          <section className="wrap openkeys-narrow">
            <div className="page-heading"><span className="eyebrow">apiToken</span><h1 className="p-h1">Вход в админку</h1></div>
            <form className="card" onSubmit={login}>
              <div className="field"><label htmlFor="user">Логин</label><input id="user" name="user" autoComplete="username" /></div>
              <div className="field"><label htmlFor="password">Пароль</label><input id="password" name="password" type="password" autoComplete="current-password" /></div>
              {error ? <div className="banner banner-error">{error}</div> : null}
              <button className="btn btn-primary" type="submit" disabled={busy}>{busy ? "Проверяем…" : "Войти"}</button>
            </form>
          </section>
        </div></div>
      </AppShell>
    );
  }

  const firstBatchNumber = batchTotal === 0 ? 0 : batchOffset + 1;
  const lastBatchNumber = Math.min(batchOffset + batchLimit, batchTotal);

  function batchWorkspace(batch: BatchRow) {
    const stock = keys.filter((key) => key.status === "stock");
    const history = keys.filter((key) => key.status === "delivered");
    const handoverKeys = stock.filter((key) => key.secret && key.enabled);

    return (
      <section className="openkeys-batch-workspace" aria-label={`Открытая партия ${batch.label ?? batch.id}`}>
        <div className="openkeys-workspace-head">
          <div><span className="eyebrow">Открытая партия</span><h2>{batch.label ?? "Без метки"}</h2><p>{batch.createdAt.slice(0, 10)} · {batch.faceValue} · {batch.pricingContract === "official_1_to_1" ? "1:1 official" : "legacy"} · {keys.length} ключей в системе</p></div>
          <button className="btn btn-ghost btn-sm" type="button" onClick={() => chooseBatch(null)}>Скрыть партию</button>
        </div>

        <div className="banner openkeys-universal-banner"><b>Один универсальный ключ</b><span>Claude: {UNIVERSAL_CONNECTIONS.claude.baseUrl} · GPT: {UNIVERSAL_CONNECTIONS.openai.baseUrl} · профиль: {OPENKEYS_PUBLIC_ORIGIN}/profile/…</span></div>

        <div className="dsec-head analytics-heading openkeys-stock-head">
          <div><h2>Ключи на складе · {stock.length}</h2><p>{handoverKeys.length} готовы к продаже; отключённые остаются видимыми. Полная инструкция — в сообщении покупателю.</p></div>
          <div className="openkeys-group-actions">
            {handoverKeys.length ? <><CopyButton value={handoverKeys.map((key) => universalKeyHandoverText(key)).join("\n\n———\n\n")} label={`Сообщения · ${handoverKeys.length}`} primary /><CopyButton value={handoverKeys.flatMap((key) => key.secret ? [key.secret] : []).join("\n")} label="Только ключи" /></> : null}
            {stock.length ? <button className="btn btn-ghost btn-sm openkeys-danger" type="button" disabled={busy} onClick={() => void removeBatchStock(stock.length)}>Удалить остаток</button> : null}
          </div>
        </div>
        {stock.length === 0 ? <div className="empty-box">В этой партии на складе ничего не осталось</div> : <div className="openkeys-key-grid">{stock.map((key) => <KeyCard key={key.id} keyRow={key} busy={busy} onUpdate={updateKey} />)}</div>}

        <div className="dsec-head analytics-heading openkeys-history-head"><div><h2>История партии · {history.length}</h2><p>Выданные ключи остаются видимыми по маске, метке и ссылке на профиль.</p></div></div>
        <div className="table-scroll openkeys-history-table" role="region" tabIndex={0} aria-label="История выбранной партии">
          <table className="mtable"><thead><tr><th>Ключ</th><th>Метка</th><th>Состояние</th><th className="tnum">Номинал</th><th className="tnum">Выпущен</th><th className="tnum">Выдан</th><th>Профиль</th></tr></thead>
            <tbody>{history.length === 0 ? <tr><td colSpan={7} className="empty-cell">В этой партии ещё ничего не выдано</td></tr> : history.map((key) => <tr key={key.id}><td><code className="key-mask">{key.keyMasked}</code></td><td><b>{key.label ?? "Без метки"}</b></td><td><span className={`pill ${key.enabled ? "pill-good" : "pill-muted"}`}>{key.enabled ? "выдан" : "отключён"}</span></td><td className="tnum">{key.faceValue}</td><td className="tnum">{key.createdAt.slice(0, 10)}</td><td className="tnum">{key.deliveredAt?.slice(0, 10) ?? "—"}</td><td><a href={key.viewUrl} target="_blank" rel="noreferrer">открыть ↗</a></td></tr>)}</tbody>
          </table>
        </div>
      </section>
    );
  }

  return (
    <AppShell
      section="stock"
      title="Партии и склад"
      actions={
        <>
          <button className="btn btn-primary btn-sm" type="button" onClick={() => setShowIssueForm((open) => !open)}>
            {showIssueForm ? "Закрыть форму" : "Выпустить партию"}
          </button>
          <button className="btn btn-ghost btn-sm" type="button" onClick={logout}>Выйти</button>
        </>
      }
    >
      <div className="app-body"><div className="app-body-in">
        {error ? <div className="banner banner-error">{error}</div> : null}

        <div className="openkeys-admin-intro">
          <div>
            <span className="eyebrow">Рабочее место продавца</span>
            <h1>Каждая продажа остаётся в своей партии</h1>
            <p>Откройте выпуск ниже: склад, массовое копирование и история появятся сразу под его строкой. Повторный клик скрывает партию.</p>
          </div>
          <span className="chip">{admin ?? "admin"}</span>
        </div>

        <div className="ov-stats bill4 openkeys-admin-stats">
          <div className="ovstat"><span className="dlabel">Партий</span><b className="num">{batchTotal}</b><span className="dtrend">доступен поиск и страницы</span></div>
          <div className="ovstat"><span className="dlabel">На складе</span><b className="num accent">{totals.stock}</b><span className="dtrend">ещё не выданы</span></div>
          <div className="ovstat"><span className="dlabel">Выдано</span><b className="num">{totals.delivered}</b><span className="dtrend">секреты уже удалены</span></div>
          <div className="ovstat"><span className="dlabel">Отключено</span><b className="num">{totals.disabled}</b><span className="dtrend">не принимают запросы</span></div>
        </div>

        {showIssueForm ? (
          <form className="card openkeys-issue-card" onSubmit={issue}>
            <div className="overview-card-head"><div><span className="overview-card-label">Новая партия</span><p className="field-hint">Метка обязательна в интерфейсе: по ней продавец находит выпуск среди сотен других.</p></div><span className="chip">1:1 official · Claude + GPT + Gemini</span></div>
            <div className="banner openkeys-universal-banner">
              <b>Фиксированная экономика: 1:1 по официальной цене</b>
              <span>Номинал равен фактическому engine balance. Скидка и множитель не настраиваются.</span>
              <span>
                {issuanceAuthority.ready
                  ? `Активный каталог: ${issuanceAuthority.supportedModels.join(", ")}`
                  : "Выпуск недоступен: активный OpenKeys catalog/provider authority ещё не подтверждён."}
              </span>
            </div>
            <div className="openkeys-form-grid">
              <div className="field"><label htmlFor="faceValueUsd">Номинал ключа, $</label><input id="faceValueUsd" name="faceValueUsd" defaultValue="50" inputMode="numeric" required /></div>
              <div className="field"><label htmlFor="quantity">Количество</label><input id="quantity" name="quantity" defaultValue="1" inputMode="numeric" required /><span className="field-hint">до 100 за раз</span></div>
              <div className="field"><label htmlFor="label">Метка партии</label><input id="label" name="label" placeholder="funpay-30-07-evening" maxLength={200} required /><span className="field-hint">площадка, дата или смена</span></div>
            </div>
            <button className="btn btn-primary" type="submit" disabled={busy || !issuanceAuthority.ready}>{busy ? "Выпускаем…" : "Создать партию"}</button>
          </form>
        ) : null}

        <section className="dsec openkeys-batches-section">
          <div className="dsec-head analytics-heading"><div><h2>Партии</h2><p>Фильтр только по партии — группировки по номиналу больше нет.</p></div></div>
          <form
            className="openkeys-batch-toolbar"
            onSubmit={(event) => {
              event.preventDefault();
              chooseBatch(null);
              void refresh({ batchId: null, offset: 0, query: batchQueryDraft.trim() });
            }}
          >
            <input value={batchQueryDraft} onChange={(event) => setBatchQueryDraft(event.target.value)} placeholder="Метка или ID партии…" maxLength={80} />
            <button className="btn btn-primary btn-sm" type="submit">Найти</button>
            {batchQuery ? <button className="btn btn-ghost btn-sm" type="button" onClick={() => { setBatchQueryDraft(""); chooseBatch(null); void refresh({ batchId: null, offset: 0, query: "" }); }}>Сбросить</button> : null}
          </form>
          <div className="table-scroll" role="region" tabIndex={0} aria-label="Партии ключей">
            <table className="mtable openkeys-batch-table">
              <thead><tr><th>Партия</th><th>Контракт</th><th>Дата</th><th className="tnum">Номинал</th><th className="tnum">Всего</th><th className="tnum">Склад</th><th className="tnum">Выдано</th><th className="tnum">Отключено</th><th /></tr></thead>
              <tbody>
                {batches.length === 0 ? <tr><td colSpan={9} className="empty-cell">{batchQuery ? "По этому фильтру партий нет" : "Пока ничего не выпускали"}</td></tr> : batches.map((batch) => {
                  const expanded = openBatch === batch.id;
                  return (
                    <Fragment key={batch.id}>
                      <tr className={expanded ? "is-selected" : ""}>
                        <td><b>{batch.label ?? "Без метки"}</b><code className="openkeys-batch-id">{batch.id.slice(0, 8)}</code></td>
                        <td><span className={`pill ${batch.pricingContract === "official_1_to_1" ? "pill-good" : "pill-muted"}`}>{batch.pricingContract === "official_1_to_1" ? "1:1 official" : "legacy"}</span></td><td>{batch.createdAt.slice(0, 10)}</td><td className="tnum">{batch.faceValue}</td><td className="tnum">{batch.quantity}</td><td className="tnum"><b>{batch.stockCount}</b></td><td className="tnum">{batch.deliveredCount}</td><td className="tnum">{batch.disabledCount}</td>
                        <td><button className={`btn btn-sm ${expanded ? "btn-primary" : "btn-ghost"}`} type="button" aria-expanded={expanded} aria-controls={expanded ? `openkeys-batch-${batch.id}` : undefined} onClick={() => { if (expanded) chooseBatch(null); else { chooseBatch(null); void refresh({ batchId: batch.id }); } }}>{expanded ? "Скрыть" : "Открыть"}</button></td>
                      </tr>
                      {expanded ? <tr className="openkeys-batch-detail-row"><td id={`openkeys-batch-${batch.id}`} className="openkeys-batch-detail-cell" colSpan={9}>{batchWorkspace(batch)}</td></tr> : null}
                    </Fragment>
                  );
                })}
              </tbody>
            </table>
          </div>
          <div className="openkeys-pagination">
            <span>{firstBatchNumber}–{lastBatchNumber} из {batchTotal}</span>
            <button className="btn btn-ghost btn-sm" type="button" disabled={batchOffset === 0} onClick={() => { chooseBatch(null); void refresh({ batchId: null, offset: Math.max(0, batchOffset - batchLimit) }); }}>Назад</button>
            <button className="btn btn-ghost btn-sm" type="button" disabled={batchOffset + batchLimit >= batchTotal} onClick={() => { chooseBatch(null); void refresh({ batchId: null, offset: batchOffset + batchLimit }); }}>Дальше</button>
          </div>
        </section>

      </div></div>
    </AppShell>
  );
}
