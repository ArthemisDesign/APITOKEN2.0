"use client";

import { useCallback, useEffect, useState } from "react";
import { Banner, EmptyRow, LoadingGrid, PageHead, Pill, SectionHeader, StatCard, CardGrid, TableCard } from "@/components/ui";
import { api, send } from "@/lib/api";
import { dialog } from "@/lib/dialog";
import { formatDate, nanoMoney } from "@/lib/format";
import { localeFor, useI18n } from "@/lib/i18n";
import { useResource } from "@/lib/resources";
import { toast } from "@/lib/toast";
import { bnbMoney, shortWallet } from "../helpers";
import { payoutGate, payoutRowSendable } from "../payout-safety";
import type { AdminPartner, PayoutBatch, PayoutEngineState, PayoutReport, PayoutRow } from "../types";

type LegacyPayout = { id: string; partnerId: string; amountNano: string; status: string; method: string; details: unknown; requestedAt: string; decidedAt: string | null; paidAt: string | null; adminNote: string | null };

const GATE_TEXT: Record<string, [string, string]> = {
  state_unavailable: ["Payout state is unavailable.", "Состояние выплат недоступно."],
  not_configured: ["The payout engine is not fully configured.", "Модуль выплат настроен не полностью."],
  window_closed: ["The payout window is closed.", "Окно выплат закрыто."],
  batch_not_prepared: ["Only a prepared idle batch can be sent.", "Можно отправить только подготовленный пакет."],
  accounting_not_ready: ["Refund accounting is not proven current.", "Актуальность учёта возвратов не подтверждена."],
  accounting_incomplete: ["Accounting cursors or allocations are incomplete.", "Курсоры или распределения учёта не завершены."],
  wallet_mismatch: ["The current hot wallet does not match the prepared batch.", "Текущий hot wallet не совпадает с кошельком пакета."],
  batch_inconsistent: ["The batch report is internally inconsistent.", "Отчёт пакета внутренне противоречив."],
  row_invalid: ["A payout row contains invalid money or wallet data.", "Строка выплаты содержит некорректную сумму или кошелёк."],
  broadcast_unresolved: ["A transaction is broadcast and unresolved.", "Транзакция отправлена в сеть и ещё не завершена."],
  row_state_inconsistent: ["Payout and chain states disagree.", "Статусы выплаты и блокчейна противоречат друг другу."],
  transaction_evidence_invalid: ["Transaction evidence is invalid.", "Подтверждение транзакции некорректно."],
  totals_inconsistent: ["Batch totals are inconsistent.", "Итоговые суммы пакета противоречивы."],
  nothing_sendable: ["The batch has no sendable rows.", "В пакете нет строк для отправки."],
  usdt_insufficient: ["Verified USDT balance is insufficient.", "Подтверждённого баланса USDT недостаточно."],
  bnb_insufficient: ["Verified BNB gas balance is insufficient.", "Подтверждённого BNB для gas недостаточно."],
};

function statusLabel(status: string | null, t: (en: string, ru: string) => string): string {
  if (!status || status === "pending") return t("Pending", "Ожидает");
  const labels: Record<string, [string, string]> = {
    requested: ["Requested", "Запрошена"], approved: ["Approved", "Одобрена"], paid: ["Paid", "Выплачена"], rejected: ["Rejected", "Отклонена"],
    preparing: ["Preparing", "Подготавливается"], prepared: ["Prepared", "Подготовлен"], sending: ["Sending", "Отправляется"], sent: ["Sent", "Отправлен"], failed: ["Failed", "Ошибка"], canceled: ["Canceled", "Отменён"],
    simulated: ["Simulated", "Проверена"], broadcast: ["Broadcast", "Отправлена в сеть"], confirmed: ["Confirmed", "Подтверждена"],
  };
  const label = labels[status];
  return label ? t(label[0], label[1]) : status;
}

function legacyPartnerIdentity(partnerId: string, partners: AdminPartner[]): string {
  const partner = partners.find((item) => item.id === partnerId);
  return partner?.email ?? partner?.displayName ?? (partner?.telegramUsername ? `@${partner.telegramUsername}` : partnerId.slice(0, 8));
}

export default function PartnerPayoutsPage() {
  const { lang, t } = useI18n();
  const locale = localeFor(lang);
  const { data: engine, isLoading: engineLoading, refresh: refreshEngine } = useResource<PayoutEngineState>("/partner-admin/payouts/engine");
  const { data: batchesData, isLoading: batchesLoading, refresh: refreshBatches } = useResource<{ items: PayoutBatch[] }>("/partner-admin/payouts/batches");
  const { data: history, refresh: refreshHistory } = useResource<{ items: LegacyPayout[] }>("/partner-admin/payouts");
  const { data: partnersData } = useResource<{ items: AdminPartner[] }>("/partner-admin/partners");
  const [report, setReport] = useState<PayoutReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const openBatch = useCallback(async (id: string) => {
    try { setReport(await api<PayoutReport>(`/partner-admin/payouts/batches/${id}`)); }
    catch (cause) { setError(cause instanceof Error ? cause.message : t("Could not load batch.", "Не удалось загрузить пакет.")); }
  }, [t]);

  useEffect(() => {
    if (report || !batchesData) return;
    const active = batchesData.items.find((item) => item.status === "sending" || item.status === "prepared");
    if (!active) return;
    let canceled = false;
    void api<PayoutReport>(`/partner-admin/payouts/batches/${active.id}`)
      .then((nextReport) => { if (!canceled) setReport(nextReport); })
      .catch((cause: unknown) => {
        if (!canceled) setError(cause instanceof Error ? cause.message : t("Could not load batch.", "Не удалось загрузить пакет."));
      });
    return () => { canceled = true; };
  }, [batchesData, report, t]);

  useEffect(() => {
    if (report?.batch.status !== "sending") return;
    const id = report.batch.id;
    const timer = window.setTimeout(() => void openBatch(id), 2500);
    return () => window.clearTimeout(timer);
  }, [openBatch, report]);

  async function act<T>(operation: () => Promise<T>, success?: string): Promise<T | null> {
    setBusy(true); setError(null);
    try { const result = await operation(); if (success) toast(success); return result; }
    catch (cause) { setError(cause instanceof Error ? cause.message : t("Action failed.", "Не удалось выполнить действие.")); return null; }
    finally { setBusy(false); }
  }
  function refreshAll() { refreshEngine(); refreshBatches(); refreshHistory(); }

  async function prepare() {
    const next = await act(() => send<PayoutReport>("/partner-admin/payouts/prepare", "POST", {}), t("Batch prepared", "Пакет подготовлен"));
    if (next) setReport(next); refreshAll();
  }
  async function sendBatch() {
    if (!engine || !report) return;
    const gate = payoutGate(engine, report);
    if (!gate.allowed) { const label = GATE_TEXT[gate.reason]; setError(label ? t(label[0], label[1]) : gate.reason); return; }
    const confirmation = await dialog({ title: t("Send on-chain payouts", "Отправить выплаты в блокчейн"), message: t(`Send ${gate.sendableCount} transfers totalling ${nanoMoney(report.chain.requiredUsdtNano)} from the pinned hot wallet? This is irreversible. Type SEND.`, `Отправить ${gate.sendableCount} переводов на ${nanoMoney(report.chain.requiredUsdtNano)} с закреплённого hot wallet? Действие необратимо. Введите SEND.`), fields: [{ name: "confirm", label: "SEND" }], confirmLabel: t("Send payouts", "Отправить выплаты"), danger: true });
    if (confirmation?.confirm !== "SEND") return;
    const next = await act(() => send<PayoutReport>(`/partner-admin/payouts/batches/${report.batch.id}/send`, "POST", {}));
    if (next) setReport(next); refreshAll();
  }
  async function sendRow(row: PayoutRow) {
    if (!report || !payoutRowSendable(row)) return;
    const next = await act(() => send(`/partner-admin/payouts/rows/${row.id}/send`, "POST", {}));
    if (next) await openBatch(report.batch.id); refreshAll();
  }
  async function releaseRow(row: PayoutRow) {
    if (!report) return;
    const confirmation = await dialog({ title: t("Release failed payout", "Освободить неудачную выплату"), message: t("Only release when the transfer did not reach the chain. The amount returns to the partner's next window. Type RELEASE.", "Освобождайте только если перевод не попал в блокчейн. Сумма вернётся в следующее окно партнёра. Введите RELEASE."), fields: [{ name: "confirm", label: "RELEASE" }], confirmLabel: t("Release", "Освободить"), danger: true });
    if (confirmation?.confirm !== "RELEASE") return;
    const next = await act(() => send(`/partner-admin/payouts/rows/${row.id}/release`, "POST", {}), t("Balance released", "Баланс освобождён"));
    if (next) await openBatch(report.batch.id); refreshAll();
  }
  async function cancelBatch() {
    if (!report) return;
    const confirmation = await dialog({ title: t("Cancel prepared batch", "Отменить подготовленный пакет"), message: t("Unsent rows will be removed and their balances released. Type CANCEL.", "Неотправленные строки будут удалены, а балансы освобождены. Введите CANCEL."), fields: [{ name: "confirm", label: "CANCEL" }], confirmLabel: t("Cancel batch", "Отменить пакет"), danger: true });
    if (confirmation?.confirm !== "CANCEL") return;
    const result = await act(() => send(`/partner-admin/payouts/batches/${report.batch.id}/cancel`, "POST", {}), t("Batch canceled", "Пакет отменён"));
    if (result) setReport(null); refreshAll();
  }
  async function rejectLegacy(payout: LegacyPayout) {
    const result = await dialog({ title: t("Reject legacy payout", "Отклонить legacy-выплату"), message: t("This path only rejects old manual requests. New payouts must use a fenced on-chain batch.", "Этот путь только отклоняет старые ручные заявки. Новые выплаты выполняются через защищённый on-chain пакет."), fields: [{ name: "note", label: t("Required reason", "Обязательная причина") }], confirmLabel: t("Reject", "Отклонить"), danger: true });
    const note = result?.note.trim(); if (!note) return;
    const next = await act(() => send(`/partner-admin/payouts/${payout.id}/decision`, "POST", { action: "reject", note }), t("Legacy request rejected", "Legacy-заявка отклонена"));
    if (next) refreshHistory();
  }

  if ((engineLoading && !engine) || (batchesLoading && !batchesData)) return <><PageHead title={t("Partner payouts", "Выплаты партнёрам")} /><LoadingGrid label={t("Loading payout controls", "Загрузка управления выплатами")} /></>;
  const batches = batchesData?.items ?? [];
  const gate = payoutGate(engine ?? null, report);
  const gateLabel = gate.allowed ? null : GATE_TEXT[gate.reason];

  return <>
    <PageHead title={t("Partner payouts", "Выплаты партнёрам")} sub={t("Prepare, verify and send fenced USDT BEP-20 batches", "Подготовка, проверка и отправка защищённых пакетов USDT BEP-20")} badge={<Pill kind={engine?.configured ? "ok" : "bad"}>{engine?.configured ? t("configured", "настроено") : t("not configured", "не настроено")}</Pill>} />
    <div aria-live="polite">{error ? <Banner kind="bad" title={t("Payout action blocked", "Действие с выплатами заблокировано")}>{error}</Banner> : null}</div>
    <CardGrid><StatCard label={t("Window", "Окно")} value={engine?.window.open ? t("OPEN", "ОТКРЫТО") : t("CLOSED", "ЗАКРЫТО")} hint={engine?.window.open ? formatDate(engine.window.closesAt, true, locale) : formatDate(engine?.window.opensAt, true, locale)} /><StatCard label={t("USDT required", "Требуется USDT")} value={nanoMoney(report?.chain.requiredUsdtNano)} hint={`${t("balance", "баланс")} ${nanoMoney(report?.chain.usdtBalanceNano)}`} /><StatCard label={t("BNB gas", "BNB для gas")} value={bnbMoney(report?.chain.requiredBnbWei)} hint={`${t("balance", "баланс")} ${bnbMoney(report?.chain.bnbBalanceWei)}`} /><StatCard label={t("Active batch", "Активный пакет")} value={report ? statusLabel(report.batch.status, t) : "—"} hint={report ? `${report.batch.recipientCount} · ${nanoMoney(report.batch.totalNano)}` : t("prepare a fresh batch", "подготовьте новый пакет")} /></CardGrid>
    <div className="partner-payout-actions"><button type="button" className="btn" disabled={busy || Boolean(report && ["prepared", "sending"].includes(report.batch.status))} onClick={prepare}>{t("Prepare batch", "Подготовить пакет")}</button><button type="button" className="btn warn" disabled={busy || !gate.allowed} onClick={sendBatch}>{t("Send batch", "Отправить пакет")}</button>{report?.batch.status === "prepared" ? <button type="button" className="btn bad" disabled={busy} onClick={cancelBatch}>{t("Cancel", "Отменить")}</button> : null}<button type="button" className="btn ghost" disabled={busy} onClick={refreshAll}>{t("Refresh proofs", "Обновить проверки")}</button></div>
    {report && !gate.allowed && gateLabel ? <Banner kind="warn" title={t("Send disabled", "Отправка заблокирована")}>{t(gateLabel[0], gateLabel[1])}</Banner> : null}
    {report?.accounting && !report.accounting.ready ? <Banner kind="bad" title={t("Accounting not ready", "Учёт не готов")}>{report.accounting.reasons.join(" · ")}</Banner> : null}

    <SectionHeader title={t("Selected batch", "Выбранный пакет")} sub={report ? `${report.batch.id} · hot wallet ${report.batch.hotWalletAddress ? shortWallet(report.batch.hotWalletAddress) : "—"}` : t("Select a batch below", "Выберите пакет ниже")} />
    <TableCard><table><thead><tr><th className="left">{t("Partner", "Партнёр")}</th><th>{t("Amount", "Сумма")}</th><th className="left">{t("Wallet", "Кошелёк")}</th><th>{t("Payout state", "Статус выплаты")}</th><th>{t("Chain state", "Статус сети")}</th><th className="left">{t("Evidence / error", "Подтверждение / ошибка")}</th><th><span className="sr-only">{t("Actions", "Действия")}</span></th></tr></thead><tbody>
      {report?.rows.length ? report.rows.map((row) => <tr key={row.id}><td className="left"><b translate="no">{row.partner}</b></td><td><b>{nanoMoney(row.amountNano)}</b></td><td className="left mono" translate="no" title={row.walletAddress ?? ""}>{row.walletAddress ? shortWallet(row.walletAddress) : "—"}</td><td><Pill kind={row.status === "paid" ? "ok" : row.status === "rejected" ? "bad" : "warn"}>{statusLabel(row.status, t)}</Pill></td><td><Pill kind={row.chainStatus === "confirmed" ? "ok" : row.chainStatus === "failed" ? "bad" : row.chainStatus === "broadcast" ? "warn" : ""}>{statusLabel(row.chainStatus, t)}</Pill></td><td className="left"><div className="json" translate="no" title={row.txHash ?? row.chainError ?? ""}>{row.txHash ?? row.chainError ?? "—"}</div></td><td><div className="actions">{payoutRowSendable(row) ? <button type="button" className="btn" disabled={busy || !gate.allowed} onClick={() => sendRow(row)}>{row.chainStatus === "failed" ? t("Retry", "Повторить") : t("Send", "Отправить")}</button> : null}{row.chainStatus === "failed" ? <button type="button" className="btn bad" disabled={busy} onClick={() => releaseRow(row)}>{t("Release", "Освободить")}</button> : null}</div></td></tr>) : <EmptyRow columns={7} text={t("No selected payout rows", "Нет выбранных строк выплат")} />}
    </tbody></table></TableCard>

    <SectionHeader title={t("Batch history", "История пакетов")} sub={`${batches.length}`} />
    <TableCard><table><thead><tr><th>{t("Status", "Статус")}</th><th>{t("Amount", "Сумма")}</th><th>{t("Recipients", "Получатели")}</th><th className="left">Hot wallet</th><th>{t("Created", "Создан")}</th><th>{t("Completed", "Завершён")}</th><th><span className="sr-only">{t("Open", "Открыть")}</span></th></tr></thead><tbody>{batches.length ? batches.map((batch) => <tr key={batch.id}><td><Pill kind={batch.status === "sent" ? "ok" : batch.status === "failed" ? "bad" : batch.status === "prepared" || batch.status === "sending" ? "warn" : ""}>{statusLabel(batch.status, t)}</Pill></td><td>{nanoMoney(batch.totalNano)}</td><td>{batch.recipientCount}</td><td className="left mono" translate="no">{batch.hotWalletAddress ? shortWallet(batch.hotWalletAddress) : "—"}</td><td>{formatDate(batch.createdAt, true, locale)}</td><td>{formatDate(batch.completedAt, true, locale)}</td><td><button type="button" className="btn ghost" onClick={() => openBatch(batch.id)}>{t("Open", "Открыть")}</button></td></tr>) : <EmptyRow columns={7} text={t("No payout batches", "Пакетов выплат нет")} />}</tbody></table></TableCard>

    <details><summary>{t("Legacy manual payout requests", "Старые ручные заявки на выплату")} · {history?.items.length ?? 0}</summary><TableCard><table><thead><tr><th className="left">Email</th><th>{t("Amount", "Сумма")}</th><th>{t("Status", "Статус")}</th><th>{t("Requested", "Запрошена")}</th><th className="left">{t("Note", "Комментарий")}</th><th><span className="sr-only">{t("Actions", "Действия")}</span></th></tr></thead><tbody>{history?.items.length ? history.items.map((item) => <tr key={item.id}><td className="left" translate="no">{legacyPartnerIdentity(item.partnerId, partnersData?.items ?? [])}</td><td>{nanoMoney(item.amountNano)}</td><td><Pill kind={item.status === "paid" ? "ok" : item.status === "rejected" ? "bad" : "warn"}>{statusLabel(item.status, t)}</Pill></td><td>{formatDate(item.requestedAt, true, locale)}</td><td className="left partner-request-note">{item.adminNote ?? "—"}</td><td>{item.status === "requested" ? <button type="button" className="btn bad" onClick={() => rejectLegacy(item)}>{t("Reject legacy", "Отклонить legacy")}</button> : "—"}</td></tr>) : <EmptyRow columns={6} text={t("No legacy payout requests", "Старых заявок на выплату нет")} />}</tbody></table></TableCard></details>
  </>;
}
