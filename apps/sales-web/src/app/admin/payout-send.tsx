"use client";

import { useCallback, useEffect, useState } from "react";
import {
  api,
  ApiError,
  formatDate,
  formatUsd,
  type PayoutBatchDto,
  type PayoutEngine,
  type PayoutReportDto,
  type PayoutRowDto,
} from "@/lib/api";
import { evaluatePayoutSendGate, isSendablePayoutRow } from "@/lib/payout-safety";
import { Badge, Button, Card, EmptyState, Loading, Notice } from "@/components/ui";
import { localeFor, useI18n } from "@/components/i18n";

function adminHeaders(key: string): Record<string, string> {
  return key ? { "x-sales-admin-key": key } : {};
}

const faint = { color: "var(--text-faint)" } as const;
const mono = { fontFamily: "var(--font-mono, monospace)", fontSize: 12 } as const;

function shortAddr(a: string | null): string {
  if (!a) return "—";
  return a.length > 14 ? `${a.slice(0, 8)}…${a.slice(-6)}` : a;
}
function bnb(wei: string | null): string {
  if (!wei || !/^\d+$/.test(wei)) return "—";
  const v = BigInt(wei);
  const whole = v / 10n ** 18n;
  const frac = ((v % 10n ** 18n) / 10n ** 14n).toString().padStart(4, "0");
  return `${whole}.${frac} BNB`;
}
type Translate = (en: string, ru: string) => string;

function fmtWindow(w: PayoutEngine["window"], t: Translate, locale: string): string {
  if (w.enforced === false) return t("gate OFF (test mode) — sending allowed anytime", "контроль выключен (тестовый режим) — отправка доступна всегда");
  if (w.open) return t(`open until ${formatDate(w.closesAt, locale)}`, `открыто до ${formatDate(w.closesAt, locale)}`);
  return t("closed — sending is disabled outside the 3-day payout window", "закрыто — отправка недоступна вне трёхдневного окна выплат");
}

function chainBadge(row: PayoutRowDto, t: Translate): React.ReactNode {
  const s = row.status === "paid" ? "paid" : row.chainStatus ?? "pending";
  const tone = s === "paid" ? "green" : s === "failed" ? "red" : s === "broadcast" ? "yellow" : undefined;
  const label: Record<string, string> = {
    paid: t("paid", "выплачено"),
    failed: t("failed", "ошибка"),
    broadcast: t("broadcast", "отправлено в сеть"),
    pending: t("pending", "ожидает"),
    simulated: t("simulated", "проверено"),
    confirmed: t("confirmed", "подтверждено"),
  };
  return <Badge tone={tone as "green" | "red" | "yellow" | undefined}>{label[s] ?? s}</Badge>;
}

function batchStatus(status: PayoutBatchDto["status"], t: Translate): string {
  const labels: Record<PayoutBatchDto["status"], string> = {
    preparing: t("preparing", "подготавливается"),
    prepared: t("prepared", "подготовлен"),
    sending: t("sending", "отправляется"),
    sent: t("sent", "отправлен"),
    failed: t("failed", "ошибка"),
    canceled: t("canceled", "отменён"),
  };
  return labels[status];
}

function invalidAddressReason(reason: string, t: Translate): string {
  if (reason === "zero address") return t("zero address", "нулевой адрес");
  return t(reason, "невалидный адрес или неверная контрольная сумма");
}

const PAYOUT_GATE_REASON_RU: Record<string, string> = {
  "Payout state is unavailable. Refresh before sending.": "Состояние выплат недоступно. Обновите данные перед отправкой.",
  "The payout engine is not fully configured.": "Модуль выплат настроен не полностью.",
  "The payout window is closed.": "Окно выплат закрыто.",
  "Only a prepared, idle batch can be sent.": "Можно отправить только подготовленный пакет без активной обработки.",
  "Partner refund accounting is not proven current.": "Актуальность учёта возвратов партнёров не подтверждена.",
  "Partner refund cursors or allocations are incomplete.": "Курсоры или распределения возвратов партнёров не завершены.",
  "The current hot wallet does not match the wallet pinned to this batch.": "Текущий горячий кошелёк не совпадает с кошельком, закреплённым за пакетом.",
  "The payout wallet identity is unavailable or inconsistent.": "Идентичность кошелька выплат недоступна или противоречива.",
  "The batch has no payout rows.": "В пакете нет строк выплат.",
  "The batch recipient count is inconsistent.": "Количество получателей в пакете противоречиво.",
  "The batch total is unavailable or invalid.": "Итоговая сумма пакета недоступна или некорректна.",
  "A payout row has an unknown state.": "Строка выплаты имеет неизвестное состояние.",
  "A payout row has invalid money or wallet data.": "В строке выплаты некорректна сумма или адрес кошелька.",
  "A transaction is still broadcast and unresolved. Wait for confirmation.": "Транзакция отправлена в сеть, но ещё не завершена. Дождитесь подтверждения.",
  "A payout row has inconsistent payment and chain states.": "Состояния выплаты и блокчейна противоречат друг другу.",
  "A rejected payout row is marked confirmed.": "Отклонённая выплата отмечена подтверждённой.",
  "A payout row has inconsistent transaction evidence.": "Данные подтверждения транзакции противоречивы.",
  "The batch total does not equal its payout rows.": "Итог пакета не равен сумме его строк.",
  "The required USDT total is unavailable or inconsistent.": "Требуемая сумма USDT недоступна или противоречива.",
  "The batch has no sendable payout rows.": "В пакете нет строк, готовых к отправке.",
  "The hot wallet has no verified sufficient USDT balance.": "Достаточный баланс USDT горячего кошелька не подтверждён.",
  "The hot wallet has no verified sufficient BNB gas balance.": "Достаточный баланс BNB для газа не подтверждён.",
};

function gateReason(reason: string, t: Translate): string {
  return t(reason, PAYOUT_GATE_REASON_RU[reason] ?? reason);
}

const ACCOUNTING_REASON_RU: Record<string, string> = {
  "usage cursor is behind its source head": "курсор расходов отстаёт от источника",
  "funding-lot cursor is behind its source head": "курсор источников финансирования отстаёт",
  "payment-reversal cursor is behind its source head": "курсор возвратов платежей отстаёт",
  "usage funding allocation is incomplete": "распределение финансирования расходов не завершено",
  "commission funding slices are incomplete": "срезы финансирования комиссии не завершены",
  "a payment reversal is not fully reflected": "возврат платежа отражён не полностью",
};

function accountingReason(reason: string, t: Translate): string {
  return t(reason, ACCOUNTING_REASON_RU[reason] ?? reason);
}

export function PayoutSendTab({ adminKey }: { adminKey: string }) {
  const { lang, t } = useI18n();
  const locale = localeFor(lang);
  const [engine, setEngine] = useState<PayoutEngine | null>(null);
  const [batches, setBatches] = useState<PayoutBatchDto[] | null>(null);
  const [report, setReport] = useState<PayoutReportDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const loadEngine = useCallback(async () => {
    try {
      setEngine(await api<PayoutEngine>("/v1/admin/payouts/engine", { headers: adminHeaders(adminKey) }));
      const res = await api<{ items: PayoutBatchDto[] }>("/v1/admin/payouts/batches", { headers: adminHeaders(adminKey) });
      setBatches(res.items);
      // auto-open the newest non-terminal batch
      const active = res.items.find((b) => b.status === "prepared" || b.status === "sending");
      if (active) await openBatch(active.id);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("Failed to load payout engine.", "Не удалось загрузить модуль выплат."));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [adminKey, t]);

  const openBatch = useCallback(async (id: string) => {
    try {
      setReport(await api<PayoutReportDto>(`/v1/admin/payouts/batches/${id}`, { headers: adminHeaders(adminKey) }));
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("Failed to load batch.", "Не удалось загрузить пакет."));
    }
  }, [adminKey, t]);

  useEffect(() => { void loadEngine(); }, [loadEngine]);

  // live progress while a batch is sending: fast right after visible movement,
  // backing off to a quiet cadence while the chain is quiet, fast again after
  // any change — instead of a fixed 4s beat that hammers the API while a long
  // batch just waits for confirmations
  useEffect(() => {
    if (report?.batch.status !== "sending") return;
    const batchId = report.batch.id;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let cancelled = false;
    let delay = 2_000;
    let lastFrame = "";

    const tick = async () => {
      try {
        const next = await api<PayoutReportDto>(`/v1/admin/payouts/batches/${batchId}`, { headers: adminHeaders(adminKey) });
        if (cancelled) return;
        const frame = JSON.stringify(next.rows.map((r) => [r.status, r.chainStatus, r.txHash]));
        delay = frame !== lastFrame ? 2_000 : Math.min(delay * 2, 15_000);
        lastFrame = frame;
        setReport(next);
      } catch {
        // transient API failure — keep the current report, retry on the slow cadence
        delay = Math.min(delay * 2, 15_000);
      }
      if (!cancelled) timer = setTimeout(() => void tick(), delay);
    };
    timer = setTimeout(() => void tick(), delay);
    return () => { cancelled = true; if (timer) clearTimeout(timer); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [report?.batch.status, report?.batch.id, adminKey]);

  async function act<T>(fn: () => Promise<T>): Promise<T | null> {
    setBusy(true); setError(null);
    try { return await fn(); }
    catch (err) { setError(err instanceof ApiError ? err.message : t("Action failed.", "Не удалось выполнить действие.")); return null; }
    finally { setBusy(false); }
  }

  async function prepare() {
    const r = await act(() => api<PayoutReportDto>("/v1/admin/payouts/prepare", { method: "POST", headers: adminHeaders(adminKey) }));
    if (r) { setReport(r); void loadEngine(); }
  }
  async function sendAll() {
    if (!report) return;
    const gate = evaluatePayoutSendGate(engine, report);
    if (!gate.allowed) { setError(gateReason(gate.reason, t)); return; }
    if (!confirm(t(
      `Send ${gate.sendableCount} payouts totalling ${formatUsd(report.chain.requiredUsdtNano)} from the hot wallet? This is irreversible.`,
      `Отправить ${gate.sendableCount} выплат на сумму ${formatUsd(report.chain.requiredUsdtNano)} с горячего кошелька? Это действие необратимо.`,
    ))) return;
    const r = await act(() => api<PayoutReportDto>(`/v1/admin/payouts/batches/${report.batch.id}/send`, { method: "POST", headers: adminHeaders(adminKey) }));
    if (r) setReport(r);
    void loadEngine();
  }
  async function sendOne(row: PayoutRowDto) {
    if (!report) return;
    const gate = evaluatePayoutSendGate(engine, report);
    if (!gate.allowed || !isSendablePayoutRow(row)) {
      setError(gate.allowed ? t("This payout row is not in a sendable state.", "Эта строка выплаты не готова к отправке.") : gateReason(gate.reason, t));
      return;
    }
    await act(() => api(`/v1/admin/payouts/rows/${row.id}/send`, { method: "POST", headers: adminHeaders(adminKey) }));
    await openBatch(report.batch.id);
  }
  async function releaseOne(row: PayoutRowDto) {
    if (!report || !confirm(t(
      "Release this failed payout? Its balance rolls back into the partner's next window. Only do this if the transaction did NOT go on-chain.",
      "Освободить эту неудачную выплату? Баланс вернётся в следующее окно партнёра. Делайте это только если транзакция НЕ попала в блокчейн.",
    ))) return;
    await act(() => api(`/v1/admin/payouts/rows/${row.id}/release`, { method: "POST", headers: adminHeaders(adminKey) }));
    await openBatch(report.batch.id);
  }
  async function cancel() {
    if (!report || !confirm(t(
      "Cancel this prepared batch? Unsent rows are removed and balances freed.",
      "Отменить подготовленный пакет? Неотправленные строки будут удалены, а балансы освобождены.",
    ))) return;
    await act(() => api(`/v1/admin/payouts/batches/${report.batch.id}/cancel`, { method: "POST", headers: adminHeaders(adminKey) }));
    setReport(null); void loadEngine();
  }

  if (!engine || !batches) return error ? <Notice kind="error">{error}</Notice> : <Loading />;

  const windowOpen = engine.window.open;
  const b = report?.batch;
  const c = report?.chain;
  const sendGate = evaluatePayoutSendGate(engine, report);
  const canSend = sendGate.allowed && !busy;

  return (
    <div className="stack">
      {error ? <Notice kind="error">{error}</Notice> : null}

      {/* Engine + window status */}
      <div className="stat-grid" style={{ gridTemplateColumns: "repeat(3, 1fr)" }}>
        <div className="stat-card">
          <div className="stat-label">{t("Payout engine", "Модуль выплат")}</div>
          <div className="stat-value" style={{ fontSize: 18, color: engine.configured ? "var(--accent-strong,#3b5bdb)" : "#d6455a" }}>
            {engine.configured ? t("Configured", "Настроен") : t("Not configured", "Не настроен")}
          </div>
          <div className="stat-foot">USDT · BNB Chain (BEP-20)</div>
        </div>
        <div className="stat-card">
          <div className="stat-label">{t("Payout window", "Окно выплат")}</div>
          <div className="stat-value" style={{ fontSize: 18, color: windowOpen ? "#26a15e" : "#d69e2e" }}>{windowOpen ? t("OPEN", "ОТКРЫТО") : t("CLOSED", "ЗАКРЫТО")}</div>
          <div className="stat-foot">{fmtWindow(engine.window, t, locale)}</div>
        </div>
        <div className="stat-card">
          <div className="stat-label">{t("Hot wallet", "Горячий кошелёк")}</div>
          <div className="stat-value" style={{ ...mono, fontSize: 15 }}>{shortAddr(c?.hotWalletAddress ?? null)}</div>
          <div className="stat-foot">{c?.usdtBalanceNano != null ? `${formatUsd(c.usdtBalanceNano)} USDT · ${bnb(c.bnbBalanceWei)}` : t("balance shown after prepare", "баланс появится после подготовки")}</div>
        </div>
      </div>

      {!windowOpen ? (
        <Notice kind="info">
          {t("Sending is", "Отправка")} <strong>{t("physically disabled", "физически заблокирована")}</strong> {t(
            "outside the two 3-day payout windows — the server rejects any send attempt now. You can still prepare and review a batch.",
            "вне двух трёхдневных окон выплат — сейчас сервер отклонит любую попытку отправки. Пакет всё ещё можно подготовить и проверить.",
          )}
        </Notice>
      ) : null}

      {report && !sendGate.allowed ? (
        <Notice kind="info"><strong>{t("Send is disabled:", "Отправка заблокирована:")}</strong> {gateReason(sendGate.reason, t)}</Notice>
      ) : null}

      {report?.accounting ? (
        <Notice kind={report.accounting.ready ? "info" : "error"}>
          <strong>{report.accounting.ready ? t("Refund accounting current.", "Учёт возвратов актуален.") : t("Refund accounting blocks payout.", "Учёт возвратов блокирует выплату.")}</strong>{" "}
          {t("usage", "расходы")} {report.accounting.usageCursor}/{report.accounting.usageSourceHead} · {t("funding", "финансирование")} {report.accounting.fundingLotCursor}/{report.accounting.fundingLotSourceHead} · {t("reversals", "возвраты")} {report.accounting.paymentReversalCursor}/{report.accounting.paymentReversalSourceHead}
          {report.accounting.reasons.length ? ` · ${report.accounting.reasons.map((reason) => accountingReason(reason, t)).join("; ")}` : ""}
        </Notice>
      ) : null}

      {!report ? (
        <Card title={t("Prepare a payout run", "Подготовить выплату")} sub={t("Collects every active partner with a valid BEP-20 address and unpaid balance > 0, validates addresses, and checks the hot wallet — nothing is sent yet.", "Собирает всех активных партнёров с корректным BEP-20-адресом и невыплаченным балансом > 0, проверяет адреса и горячий кошелёк — отправки ещё не происходит.")}>
          <Button onClick={prepare} loading={busy} disabled={!engine.configured}>{t("Prepare payout run", "Подготовить выплату")}</Button>
          {!engine.configured ? <div style={{ marginTop: 8, fontSize: 13, ...faint }}>{t("Set the hot-wallet key + BlockRazor send RPC in server env to enable.", "Чтобы включить отправку, задайте ключ горячего кошелька и BlockRazor send RPC в серверном окружении.")}</div> : null}
        </Card>
      ) : (
        <Card
          title={t("Payout batch", "Пакет выплат") + ` · ${batchStatus(b!.status, t)}`}
          sub={t(`${b!.recipientCount} recipients · ${formatUsd(b!.totalNano)} total · gas ${b!.gasPriceGwei} gwei · prepared ${formatDate(b!.preparedAt, locale)}`, `${b!.recipientCount} получателей · всего ${formatUsd(b!.totalNano)} · газ ${b!.gasPriceGwei} gwei · подготовлен ${formatDate(b!.preparedAt, locale)}`)}
        >
          {/* balance sufficiency */}
          {c ? (
            <div style={{ display: "flex", gap: 16, flexWrap: "wrap", marginBottom: 12, fontSize: 13 }}>
              <span>{t("Needs", "Требуется")} <strong>{formatUsd(c.requiredUsdtNano)}</strong> USDT {c.sufficientUsdt === false ? <Badge tone="red">{t("insufficient", "недостаточно")}</Badge> : c.sufficientUsdt ? <Badge tone="green">{t("ok", "достаточно")}</Badge> : null}</span>
              <span>{t("Gas", "Газ")} ~<strong>{bnb(c.requiredBnbWei)}</strong> {c.sufficientBnb === false ? <Badge tone="red">{t("insufficient", "недостаточно")}</Badge> : c.sufficientBnb ? <Badge tone="green">{t("ok", "достаточно")}</Badge> : null}</span>
            </div>
          ) : null}

          <div style={{ display: "flex", gap: 8, flexWrap: "wrap", marginBottom: 14 }}>
            <Button onClick={sendAll} loading={busy} disabled={!canSend}>
              {b!.status === "sending" ? t("Sending…", "Отправка…") : t("Send all", "Отправить всё")}
            </Button>
            {b!.status === "prepared" ? <Button variant="ghost" onClick={cancel} disabled={busy}>{t("Cancel batch", "Отменить пакет")}</Button> : null}
            <Button variant="ghost" onClick={() => openBatch(b!.id)} disabled={busy}>{t("Refresh", "Обновить")}</Button>
            {!windowOpen ? <span style={{ alignSelf: "center", fontSize: 12, color: "#d69e2e" }}>{t("Send disabled — window closed", "Отправка заблокирована — окно закрыто")}</span> : null}
            {c?.sufficientUsdt === false ? <span style={{ alignSelf: "center", fontSize: 12, color: "#d6455a" }}>{t("Top up hot-wallet USDT", "Пополните USDT горячего кошелька")}</span> : null}
          </div>

          {report!.invalidAddresses.length > 0 ? (
            <Notice kind="info">
              {t(`${report!.invalidAddresses.length} partner(s) excluded for an invalid address:`, `${report!.invalidAddresses.length} партнёров исключено из-за некорректного адреса:`)} {report!.invalidAddresses.map((i) => `${i.partnerId.slice(0, 8)} (${invalidAddressReason(i.reason, t)})`).join(", ")}
            </Notice>
          ) : null}

          <div style={{ overflowX: "auto", marginTop: 8 }}>
            <table style={{ width: "100%", fontSize: 13, borderCollapse: "collapse" }}>
              <thead>
                <tr style={{ textAlign: "left", ...faint, fontSize: 11 }}>
                  <th style={{ padding: "4px 8px" }}>{t("Partner", "Партнёр")}</th>
                  <th style={{ padding: "4px 8px" }}>{t("Address", "Адрес")}</th>
                  <th style={{ padding: "4px 8px" }} className="num">{t("Amount", "Сумма")}</th>
                  <th style={{ padding: "4px 8px" }}>{t("Status", "Статус")}</th>
                  <th style={{ padding: "4px 8px" }}>Tx</th>
                  <th style={{ padding: "4px 8px" }} />
                </tr>
              </thead>
              <tbody>
                {report!.rows.map((r) => (
                  <tr key={r.id} style={{ borderTop: "1px solid var(--border)" }}>
                    <td style={{ padding: "6px 8px", fontWeight: 600 }}>{r.partner}</td>
                    <td style={{ padding: "6px 8px", ...mono }}>{shortAddr(r.walletAddress)}</td>
                    <td style={{ padding: "6px 8px", fontWeight: 600 }} className="num">{formatUsd(r.amountNano)}</td>
                    <td style={{ padding: "6px 8px" }}>{chainBadge(r, t)}{r.chainError ? <div style={{ fontSize: 11, color: "#d6455a" }}>{r.chainError.slice(0, 60)}</div> : null}</td>
                    <td style={{ padding: "6px 8px" }}>
                      {r.txHash ? <a href={`https://bscscan.com/tx/${r.txHash}`} target="_blank" rel="noreferrer" style={{ ...mono, color: "var(--accent-strong,#3b5bdb)" }}>{r.txHash.slice(0, 10)}…</a> : "—"}
                    </td>
                    <td style={{ padding: "6px 8px" }}>
                      {isSendablePayoutRow(r) || (r.status === "requested" && r.chainStatus === "failed") ? (
                        <span style={{ display: "inline-flex", gap: 4 }}>
                          {isSendablePayoutRow(r) ? (
                            <Button size="sm" variant="ghost" disabled={!canSend} onClick={() => sendOne(r)}>{r.chainStatus === "failed" ? t("Retry", "Повторить") : t("Send", "Отправить")}</Button>
                          ) : null}
                          {r.chainStatus === "failed" ? <Button size="sm" variant="ghost" disabled={busy} onClick={() => releaseOne(r)}>{t("Release", "Освободить")}</Button> : null}
                        </span>
                      ) : null}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Card>
      )}

      {/* history */}
      <Card title={t("Batch history", "История пакетов")}>
        {batches.length === 0 ? <EmptyState title={t("No payout runs yet", "Пакетов выплат пока нет")} /> : (
          <div style={{ overflowX: "auto" }}>
            <table style={{ width: "100%", fontSize: 13, borderCollapse: "collapse" }}>
              <thead><tr style={{ textAlign: "left", ...faint, fontSize: 11 }}>
                <th style={{ padding: "4px 8px" }}>{t("Created", "Создан")}</th><th style={{ padding: "4px 8px" }}>{t("Status", "Статус")}</th>
                <th style={{ padding: "4px 8px" }} className="num">{t("Recipients", "Получатели")}</th><th style={{ padding: "4px 8px" }} className="num">{t("Total", "Итого")}</th><th style={{ padding: "4px 8px" }} />
              </tr></thead>
              <tbody>
                {batches.map((bat) => (
                  <tr key={bat.id} style={{ borderTop: "1px solid var(--border)" }}>
                    <td style={{ padding: "6px 8px" }}>{formatDate(bat.createdAt, locale)}</td>
                    <td style={{ padding: "6px 8px" }}><Badge tone={bat.status === "sent" ? "green" : bat.status === "canceled" || bat.status === "failed" ? "red" : "yellow"}>{batchStatus(bat.status, t)}</Badge></td>
                    <td style={{ padding: "6px 8px" }} className="num">{bat.recipientCount}</td>
                    <td style={{ padding: "6px 8px" }} className="num">{formatUsd(bat.totalNano)}</td>
                    <td style={{ padding: "6px 8px" }}><Button size="sm" variant="ghost" onClick={() => openBatch(bat.id)}>{t("Open", "Открыть")}</Button></td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>
    </div>
  );
}
