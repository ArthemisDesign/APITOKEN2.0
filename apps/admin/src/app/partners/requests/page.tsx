"use client";

import { useRouter, useSearchParams } from "next/navigation";
import { useState, type FormEvent } from "react";
import { Banner, EmptyRow, LoadingGrid, Modal, PageHead, Pill, TableCard } from "@/components/ui";
import { send } from "@/lib/api";
import { formatDate } from "@/lib/format";
import { localeFor, useI18n } from "@/lib/i18n";
import { useResource } from "@/lib/resources";
import { toast } from "@/lib/toast";
import { parsePercentBps } from "../helpers";
import type { AdminPartnerRequest, PartnerRequestStatus, PartnerRequestType, PartnerRequestsPage } from "../types";

type Filters = { status: "" | PartnerRequestStatus; type: "" | PartnerRequestType };
type DecisionDraft = { action: "approve" | "reject"; note: string; primary: string; providers: Record<string, string> };

function formatBps(value: number | null): string {
  if (value === null) return "—";
  const pct = value / 100;
  return `${Number.isInteger(pct) ? pct : pct.toFixed(2).replace(/0+$/, "").replace(/\.$/, "")}%`;
}

function requestTitle(type: PartnerRequestType, t: (en: string, ru: string) => string): string {
  if (type === "commission_change") return t("Commission change", "Повышение комиссии");
  if (type === "b2b_conversion") return t("B2B conversion", "Перевод в B2B");
  return t("B2B pricing", "B2B-условия");
}

function statusKind(status: PartnerRequestStatus): "ok" | "warn" | "bad" | "info" | "" {
  if (status === "applied") return "ok";
  if (status === "pending") return "warn";
  if (status === "approved") return "info";
  if (status === "rejected" || status === "apply_failed") return "bad";
  return "";
}

function statusLabel(status: PartnerRequestStatus, t: (en: string, ru: string) => string): string {
  if (status === "pending") return t("Pending", "Ожидает");
  if (status === "approved") return t("Approved · applying", "Одобрено · применяется");
  if (status === "applied") return t("Applied", "Применено");
  if (status === "rejected") return t("Rejected", "Отклонено");
  return t("Delivery failed", "Ошибка применения");
}

function effectLabel(status: NonNullable<AdminPartnerRequest["effect"]>["status"], t: (en: string, ru: string) => string): string {
  if (status === "pending") return t("Pending", "Ожидает");
  if (status === "processing") return t("Processing", "Обрабатывается");
  if (status === "applied") return t("Applied", "Применено");
  return t("Failed", "Ошибка");
}

export default function PartnerRequestsPage() {
  const { lang, t } = useI18n();
  const locale = localeFor(lang);
  const router = useRouter();
  const search = useSearchParams();
  const statusParam = search.get("status");
  const typeParam = search.get("type");
  const filters: Filters = {
    status: statusParam === "all" ? "" : statusParam === "approved" || statusParam === "rejected" || statusParam === "applied" || statusParam === "apply_failed" || statusParam === "pending" ? statusParam : "pending",
    type: typeParam === "commission_change" || typeParam === "b2b_conversion" || typeParam === "b2b_pricing" ? typeParam : "",
  };
  const [selected, setSelected] = useState<AdminPartnerRequest | null>(null);
  const [draft, setDraft] = useState<DecisionDraft | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  function setFilter(name: "status" | "type", value: string) {
    const params = new URLSearchParams();
    const nextStatus = name === "status" ? value : filters.status;
    const nextType = name === "type" ? value : filters.type;
    params.set("status", nextStatus || "all");
    if (nextType) params.set("type", nextType);
    router.replace(`/partners/requests${params.size ? `?${params}` : ""}`, { scroll: false });
  }

  const requestParams = new URLSearchParams({ limit: "100" });
  if (filters.status) requestParams.set("status", filters.status);
  if (filters.type) requestParams.set("requestType", filters.type);
  const path = `/partner-admin/requests?${requestParams}`;
  const { data, isLoading, refresh } = useResource<PartnerRequestsPage>(path);

  function showError(message: string, fieldId?: string) {
    setError(message);
    if (fieldId) window.requestAnimationFrame(() => document.getElementById(fieldId)?.focus());
  }

  function openDecision(request: AdminPartnerRequest, action: "approve" | "reject" = "approve") {
    const primaryBps = request.requestType === "commission_change" ? request.requestedCommissionBps : request.requestedDiscountBps;
    setSelected(request);
    setDraft({
      action,
      note: "",
      primary: primaryBps === null ? "" : String(primaryBps / 100),
      providers: Object.fromEntries(request.providerTerms.map((term) => [term.providerId, term.requestedDiscountBps === null ? "" : String(term.requestedDiscountBps / 100)])),
    });
    setError(null);
  }

  async function decide(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!selected || !draft) return;
    setError(null);
    const note = draft.note.trim();
    if (!note) return showError(t("A reviewer note is required.", "Комментарий проверяющего обязателен."), "partner-review-note");
    const body: Record<string, unknown> = { action: draft.action, note };
    if (draft.action === "approve") {
      const approvedBps = parsePercentBps(draft.primary, 10_000);
      if (approvedBps === null) return showError(t("Enter a valid approved percent.", "Введите корректный одобренный процент."), "partner-approved-percent");
      const requested = (selected.requestType === "commission_change" ? selected.requestedCommissionBps : selected.requestedDiscountBps) ?? -1;
      if (approvedBps > requested) return showError(t("The approved value cannot exceed the request.", "Одобренное значение не может превышать запрос."), "partner-approved-percent");
      if (selected.requestType === "commission_change") body.commissionBps = approvedBps;
      else {
        if (approvedBps % 100 !== 0) return showError(t("B2B discounts must be whole percents.", "B2B-скидки задаются целыми процентами."), "partner-approved-percent");
        body.discountPercent = approvedBps / 100;
        const providers: Record<string, number | null> = {};
        for (const term of selected.providerTerms) {
          const raw = draft.providers[term.providerId] ?? "base";
          if (term.requestedDiscountBps === null) { providers[term.providerId] = null; continue; }
          const providerBps = parsePercentBps(raw, 9_500);
          if (providerBps === null || providerBps % 100 !== 0 || providerBps > term.requestedDiscountBps) return showError(t(`Invalid ${term.providerId} decision.`, `Некорректное решение для ${term.providerId}.`), `partner-approved-provider-${term.providerId}`);
          providers[term.providerId] = providerBps / 100;
        }
        body.providers = providers;
      }
    }
    setBusy(true);
    try {
      await send(`/partner-admin/requests/${selected.id}/decision`, "POST", body);
      toast(draft.action === "approve" ? t("Request approved", "Заявка одобрена") : t("Request rejected", "Заявка отклонена"));
      setSelected(null); setDraft(null); refresh();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("Decision failed.", "Не удалось сохранить решение."));
    } finally { setBusy(false); }
  }

  const items = data?.items ?? [];
  const pending = items.filter((item) => item.status === "pending").length;
  if (isLoading && !data) return <><PageHead title={t("Partner requests", "Заявки партнёров")} sub={t("Loading review queue", "Загружаем очередь")} /><LoadingGrid label={t("Loading partner requests", "Загрузка заявок партнёров")} /></>;

  return <>
    <PageHead title={t("Partner requests", "Заявки партнёров")} sub={t("Commission, B2B conversion and pricing decisions", "Комиссии, перевод в B2B и индивидуальные условия")} badge={<Pill kind={pending ? "warn" : "ok"}>{pending} {t("pending", "ожидают")}</Pill>} />
    {!data ? <Banner kind="bad" title={t("Queue unavailable", "Очередь недоступна")}>{path}</Banner> : null}
    <div className="partner-request-toolbar">
      <label className="field"><span>{t("Status", "Статус")}</span><select name="requestStatus" value={filters.status} onChange={(event) => setFilter("status", event.target.value)}><option value="">{t("All", "Все")}</option><option value="pending">{t("Pending", "Ожидают")}</option><option value="approved">{t("Approved · applying", "Одобрены · применяются")}</option><option value="applied">{t("Applied", "Применены")}</option><option value="rejected">{t("Rejected", "Отклонены")}</option><option value="apply_failed">{t("Delivery failed", "Ошибка применения")}</option></select></label>
      <label className="field"><span>{t("Type", "Тип")}</span><select name="requestType" value={filters.type} onChange={(event) => setFilter("type", event.target.value)}><option value="">{t("All", "Все")}</option><option value="commission_change">{t("Commission", "Комиссия")}</option><option value="b2b_conversion">{t("B2B conversion", "Перевод в B2B")}</option><option value="b2b_pricing">{t("B2B pricing", "B2B-условия")}</option></select></label>
      <button type="button" className="btn ghost" onClick={refresh}>{t("Refresh", "Обновить")}</button>
    </div>
    <TableCard><table className="partner-requests-table"><thead><tr><th className="left">{t("Requester", "Партнёр")}</th><th>{t("Request", "Запрос")}</th><th>{t("Account", "Аккаунт")}</th><th>{t("Requested terms", "Условия")}</th><th>{t("Status", "Статус")}</th><th className="left">{t("Reason / result", "Обоснование / результат")}</th><th>{t("Created", "Создана")}</th><th><span className="sr-only">{t("Actions", "Действия")}</span></th></tr></thead><tbody>
      {items.length ? items.map((request) => <tr key={request.id}><td className="left"><b translate="no">{request.requesterEmail ?? request.requesterDisplayName ?? request.requesterPartnerId.slice(0, 8)}</b></td><td>{requestTitle(request.requestType, t)}</td><td className="left mono" translate="no">{request.customerEmail ?? "—"}</td><td><b>{formatBps(request.requestedCommissionBps ?? request.requestedDiscountBps)}</b>{request.providerTerms.length ? <div className="sub" translate="no">{request.providerTerms.map((term) => `${term.providerId} ${formatBps(term.requestedDiscountBps)}`).join(" · ")}</div> : null}</td><td><Pill kind={statusKind(request.status)}>{statusLabel(request.status, t)}</Pill>{request.effect ? <div className="sub">Commerce: {request.effect.terminal ? t("manual action", "ручное действие") : effectLabel(request.effect.status, t)} · {request.effect.attempts}</div> : null}</td><td className="left"><div className="partner-request-reason" title={request.reason}>{request.reason}</div>{request.reviewerNote ? <div className="sub partner-request-note">{request.reviewerActor}: {request.reviewerNote}</div> : null}{request.lastApplyError ? <div className="sub partner-bad partner-request-note">{request.lastApplyError}</div> : null}</td><td>{formatDate(request.createdAt, true, locale)}</td><td>{request.status === "pending" ? <div className="actions"><button type="button" className="btn" onClick={() => openDecision(request, "approve")}>{t("Review", "Рассмотреть")}</button><button type="button" className="btn bad" onClick={() => openDecision(request, "reject")}>{t("Reject", "Отклонить")}</button></div> : "—"}</td></tr>) : <EmptyRow columns={8} text={t("No requests in this view", "В этом представлении заявок нет")} />}
    </tbody></table></TableCard>

    <Modal open={Boolean(selected && draft)} wide title={selected ? requestTitle(selected.requestType, t) : ""} message={selected ? `${selected.requesterEmail ?? selected.requesterPartnerId} · ${selected.customerEmail ?? t("partner commission", "комиссия партнёра")}` : undefined} onClose={() => { if (!busy) { setSelected(null); setDraft(null); } }}>
      {selected && draft ? <form className="partner-decision-form" onSubmit={decide} noValidate>
        <div aria-live="polite">{error ? <div className="partner-form-error" role="alert">{error}</div> : null}</div>
        <div className="partner-request-full-reason"><b>{t("Partner reason", "Обоснование партнёра")}</b><p>{selected.reason}</p></div>
        <div className="partner-decision-choice" role="group" aria-label={t("Decision", "Решение")}><button type="button" className={`btn${draft.action === "approve" ? "" : " ghost"}`} aria-pressed={draft.action === "approve"} onClick={() => setDraft({ ...draft, action: "approve" })}>{t("Approve", "Одобрить")}</button><button type="button" className={`btn${draft.action === "reject" ? " bad" : " ghost"}`} aria-pressed={draft.action === "reject"} onClick={() => setDraft({ ...draft, action: "reject" })}>{t("Reject", "Отклонить")}</button></div>
        {draft.action === "approve" ? <div className="partner-approved-terms"><label className="field"><span>{selected.requestType === "commission_change" ? t("Approved commission", "Одобренная комиссия") : t("Approved base discount", "Одобренная базовая скидка")}</span><div className="percent-input"><input id="partner-approved-percent" name="approvedPercent" type="number" inputMode="decimal" autoComplete="off" min="0" max="100" step={selected.requestType === "commission_change" ? "0.01" : "1"} value={draft.primary} onChange={(event) => setDraft({ ...draft, primary: event.target.value })} disabled={busy} /><i>%</i></div><small>{t("Requested", "Запрошено")}: {formatBps(selected.requestedCommissionBps ?? selected.requestedDiscountBps)}</small></label>
          {selected.providerTerms.map((term) => <label className="field" key={term.providerId}><span translate="no">{term.providerId}</span><div className="percent-input"><input id={`partner-approved-provider-${term.providerId}`} name={`approvedProvider-${term.providerId}`} type="text" inputMode="numeric" autoComplete="off" value={term.requestedDiscountBps === null ? t("base", "база") : (draft.providers[term.providerId] ?? "")} readOnly={term.requestedDiscountBps === null} onChange={(event) => setDraft({ ...draft, providers: { ...draft.providers, [term.providerId]: event.target.value.replace(/\D/g, "") } })} disabled={busy} /><i>{term.requestedDiscountBps === null ? "" : "%"}</i></div><small>{t("Requested", "Запрошено")}: {formatBps(term.requestedDiscountBps)}</small></label>)}
        </div> : null}
        <label className="field"><span>{t("Reviewer note", "Комментарий проверяющего")}</span><textarea id="partner-review-note" name="reviewerNote" autoComplete="off" rows={4} maxLength={4000} value={draft.note} onChange={(event) => setDraft({ ...draft, note: event.target.value })} disabled={busy} placeholder={t("Why this decision is commercially safe…", "Почему это решение коммерчески обосновано…")} /></label>
        <div className="dlg-actions"><button type="button" className="btn ghost" onClick={() => { setSelected(null); setDraft(null); }} disabled={busy}>{t("Cancel", "Отмена")}</button><button type="submit" className={`btn${draft.action === "reject" ? " bad" : ""}`} disabled={busy}>{busy ? t("Saving…", "Сохраняем…") : draft.action === "approve" ? t("Approve request", "Одобрить заявку") : t("Reject request", "Отклонить заявку")}</button></div>
      </form> : null}
    </Modal>
  </>;
}
