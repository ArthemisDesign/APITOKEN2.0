"use client";

import { useCallback, useEffect, useMemo, useState, type FormEvent } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import {
  api,
  ApiError,
  formatBps,
  formatDate,
  type PartnerRequest,
  type PartnerRequestsResponse,
  type PartnerRequestStatus,
  type PartnerRequestType,
} from "@/lib/api";
import { Badge, Button, Card, EmptyState, Field, Input, Loading, Notice, Table, Textarea } from "@/components/ui";
import { localeFor, useI18n } from "@/components/i18n";
import { usePartner } from "@/components/partner-context";
import { providerLabel } from "@/components/provider-breakdown";

const STATUS_VALUES = ["", "pending", "approved", "applied", "rejected", "apply_failed"] as const;
const TYPE_VALUES = ["", "commission_change", "b2b_conversion", "b2b_pricing"] as const;

function requestTone(status: PartnerRequestStatus): "green" | "yellow" | "red" | "neutral" {
  if (status === "applied") return "green";
  if (status === "pending" || status === "approved") return "yellow";
  if (status === "rejected" || status === "apply_failed") return "red";
  return "neutral";
}

function requestStatusLabel(status: PartnerRequestStatus, t: (en: string, ru: string) => string): string {
  if (status === "pending") return t("Pending", "На рассмотрении");
  if (status === "approved") return t("Approved · applying", "Одобрено · применяется");
  if (status === "applied") return t("Applied", "Применено");
  if (status === "rejected") return t("Rejected", "Отклонено");
  return t("Delivery failed", "Ошибка применения");
}

function effectStatusLabel(status: NonNullable<PartnerRequest["effect"]>["status"], t: (en: string, ru: string) => string): string {
  if (status === "pending") return t("Pending", "Ожидает");
  if (status === "processing") return t("Processing", "Обрабатывается");
  if (status === "applied") return t("Applied", "Применено");
  return t("Failed", "Ошибка");
}

function requestIdentity(item: PartnerRequest): string {
  return item.customerEmail ?? item.requesterEmail ?? `request-${item.id.slice(0, 8)}`;
}

function bpsInput(value: number): string {
  return String(value / 100);
}

function parseCommissionPercent(value: string): number | null {
  const match = /^(0|[1-9]\d{0,2})(?:\.(\d{1,2}))?$/.exec(value.trim());
  if (!match) return null;
  const basisPoints = Number(match[1]) * 100 + Number((match[2] ?? "").padEnd(2, "0"));
  return basisPoints <= 10_000 ? basisPoints : null;
}

export default function RequestsPage() {
  const { lang, t } = useI18n();
  const locale = localeFor(lang);
  const partner = usePartner();
  const router = useRouter();
  const search = useSearchParams();
  const status = STATUS_VALUES.includes((search.get("status") ?? "") as typeof STATUS_VALUES[number])
    ? (search.get("status") ?? "") as "" | PartnerRequestStatus
    : "";
  const requestType = TYPE_VALUES.includes((search.get("type") ?? "") as typeof TYPE_VALUES[number])
    ? (search.get("type") ?? "") as "" | PartnerRequestType
    : "";
  const [items, setItems] = useState<PartnerRequest[] | null>(null);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [requestedPercent, setRequestedPercent] = useState(bpsInput(Math.min(10_000, partner.commissionBps + 500)));
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const [paging, setPaging] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  function showError(message: string, fieldId: string) {
    setError(message);
    window.requestAnimationFrame(() => document.getElementById(fieldId)?.focus());
  }

  const query = useMemo(() => {
    const params = new URLSearchParams({ limit: "25" });
    if (status) params.set("status", status);
    if (requestType) params.set("requestType", requestType);
    return params.toString();
  }, [requestType, status]);

  const load = useCallback(async () => {
    const response = await api<PartnerRequestsResponse>(`/v1/partner/requests?${query}`);
    setItems(response.items);
    setNextCursor(response.nextCursor);
  }, [query]);

  useEffect(() => {
    let cancelled = false;
    setItems(null);
    setError(null);
    (async () => {
      try {
        const response = await api<PartnerRequestsResponse>(`/v1/partner/requests?${query}`);
        if (!cancelled) {
          setItems(response.items);
          setNextCursor(response.nextCursor);
        }
      } catch (cause) {
        if (!cancelled) setError(cause instanceof ApiError ? cause.message : t("Could not load requests.", "Не удалось загрузить заявки."));
      }
    })();
    return () => { cancelled = true; };
  }, [query, t]);

  function setFilter(name: "status" | "type", value: string) {
    const params = new URLSearchParams(search.toString());
    if (value) params.set(name, value);
    else params.delete(name);
    router.replace(`/dashboard/requests${params.size ? `?${params}` : ""}`, { scroll: false });
  }

  async function submitCommission(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setError(null);
    setSuccess(null);
    const requestedCommissionBps = parseCommissionPercent(requestedPercent);
    if (requestedCommissionBps === null || requestedCommissionBps <= partner.commissionBps) {
      showError(t(
        `Enter a rate above your current ${formatBps(partner.commissionBps)} and no greater than 100%.`,
        `Укажите ставку выше текущих ${formatBps(partner.commissionBps)}, но не больше 100%.`,
      ), "request-commission-rate");
      return;
    }
    const cleanReason = reason.trim();
    if (!cleanReason) {
      showError(t("Explain the expected volume and why the higher rate is justified.", "Опишите ожидаемый объём и почему повышение обосновано."), "request-commission-reason");
      return;
    }
    setBusy(true);
    try {
      await api("/v1/partner/requests/commission", {
        method: "POST",
        headers: { "Idempotency-Key": crypto.randomUUID() },
        body: { requestedCommissionBps, reason: cleanReason },
      });
      setReason("");
      setSuccess(t("Request sent for review.", "Заявка отправлена на рассмотрение."));
      await load();
    } catch (cause) {
      setError(cause instanceof ApiError ? cause.message : t("Could not send the request.", "Не удалось отправить заявку."));
    } finally {
      setBusy(false);
    }
  }

  async function loadMore() {
    if (!nextCursor) return;
    setPaging(true);
    setError(null);
    try {
      const response = await api<PartnerRequestsResponse>(`/v1/partner/requests?${query}&cursor=${encodeURIComponent(nextCursor)}`);
      setItems((current) => [...(current ?? []), ...response.items]);
      setNextCursor(response.nextCursor);
    } catch (cause) {
      setError(cause instanceof ApiError ? cause.message : t("Could not load more requests.", "Не удалось загрузить следующие заявки."));
    } finally {
      setPaging(false);
    }
  }

  return <>
    <h1 className="page-title">{t("Requests", "Заявки")}</h1>
    <p className="page-sub">{t(
      "Request a higher direct commission or track B2B conversion and pricing reviews. Every decision and Commerce delivery state stays visible here.",
      "Запрашивайте повышение прямой комиссии и отслеживайте B2B-переводы и условия. Решение и статус применения в Commerce всегда видны здесь.",
    )}</p>

    <div className="stack">
      {error ? <Notice kind="error">{error}</Notice> : null}
      {success ? <Notice kind="success">{success}</Notice> : null}

      <Card title={t("Request a higher commission", "Запросить повышение комиссии")} sub={t(
        "The platform reviews this rate. Team parents cannot change a member's platform-funded direct commission.",
        "Эту ставку рассматривает платформа. Руководитель команды не может менять прямую комиссию участника от платформы.",
      )}>
        <form className="request-commission-form" onSubmit={submitCommission} noValidate>
          <Field label={t("Requested rate", "Желаемая ставка")} htmlFor="request-commission-rate" hint={`${t("Current", "Сейчас")}: ${formatBps(partner.commissionBps)}`}>
            <div className="input-suffix">
              <Input id="request-commission-rate" type="number" min={0} max={100} step={0.01} inputMode="decimal" autoComplete="off" value={requestedPercent} onChange={(event) => setRequestedPercent(event.target.value)} disabled={busy} />
              <span aria-hidden>%</span>
            </div>
          </Field>
          <Field label={t("Reason", "Обоснование")} htmlFor="request-commission-reason" hint={t("Volume, pipeline and commercial value", "Объём, воронка и коммерческая ценность")}>
            <Textarea id="request-commission-reason" autoComplete="off" value={reason} maxLength={4000} onChange={(event) => setReason(event.target.value)} disabled={busy} />
          </Field>
          <Button type="submit" loading={busy}>{t("Send for review", "Отправить на рассмотрение")}</Button>
        </form>
      </Card>

      <Card title={t("Request history", "История заявок")}>
        <div className="request-filters" aria-label={t("Request filters", "Фильтры заявок")}>
          <label><span>{t("Status", "Статус")}</span><select name="requestStatus" value={status} onChange={(event) => setFilter("status", event.target.value)}>
            <option value="">{t("All statuses", "Все статусы")}</option>
            <option value="pending">{t("Pending", "На рассмотрении")}</option>
            <option value="approved">{t("Approved · applying", "Одобрено · применяется")}</option>
            <option value="applied">{t("Applied", "Применено")}</option>
            <option value="rejected">{t("Rejected", "Отклонено")}</option>
            <option value="apply_failed">{t("Delivery failed", "Ошибка применения")}</option>
          </select></label>
          <label><span>{t("Type", "Тип")}</span><select name="requestType" value={requestType} onChange={(event) => setFilter("type", event.target.value)}>
            <option value="">{t("All types", "Все типы")}</option>
            <option value="commission_change">{t("Commission", "Комиссия")}</option>
            <option value="b2b_conversion">{t("B2B conversion", "Перевод в B2B")}</option>
            <option value="b2b_pricing">{t("B2B pricing", "B2B-условия")}</option>
          </select></label>
        </div>
        {!items ? <Loading label={t("Loading requests…", "Загружаем заявки…")} /> : items.length === 0 ? (
          <EmptyState title={t("No requests yet", "Заявок пока нет")}>{t("B2B requests are created from the Referrals page.", "B2B-заявки создаются на странице «Рефералы».")}</EmptyState>
        ) : <Table label={t("Partner requests", "Заявки партнёра")} head={<>
          <th>{t("Request", "Заявка")}</th><th>{t("Account", "Аккаунт")}</th><th>{t("Requested", "Запрошено")}</th><th>{t("Status", "Статус")}</th><th>{t("Decision / delivery", "Решение / применение")}</th><th>{t("Created", "Создана")}</th>
        </>}>
          {items.map((item) => <tr key={item.id}>
            <td><strong>{t(
              item.requestType === "commission_change" ? "Commission change" : item.requestType === "b2b_conversion" ? "B2B conversion" : "B2B pricing",
              item.requestType === "commission_change" ? "Повышение комиссии" : item.requestType === "b2b_conversion" ? "Перевод в B2B" : "B2B-условия",
            )}</strong><div className="field-hint request-reason" title={item.reason}>{item.reason}</div></td>
            <td><span className="identity-email" title={requestIdentity(item)} translate="no">{requestIdentity(item)}</span></td>
            <td>{item.requestedCommissionBps != null ? formatBps(item.requestedCommissionBps) : item.requestedDiscountBps != null ? formatBps(item.requestedDiscountBps) : "—"}
              {item.providerTerms.length ? <div className="field-hint" translate="no">{item.providerTerms.map((term) => `${providerLabel(term.providerId, term.providerId)} ${term.requestedDiscountBps == null ? t("base", "база") : formatBps(term.requestedDiscountBps)}`).join(" · ")}</div> : null}
            </td>
            <td><Badge tone={requestTone(item.status)}>{requestStatusLabel(item.status, t)}</Badge></td>
            <td>{item.reviewerNote ?? "—"}{item.effect ? <div className="field-hint">Commerce: {item.effect.terminal ? t("manual action required", "нужно ручное действие") : effectStatusLabel(item.effect.status, t)} · {item.effect.attempts} {t("attempts", "попыток")}</div> : null}{item.lastApplyError ? <div className="field-hint request-error">{item.lastApplyError}</div> : null}</td>
            <td>{formatDate(item.createdAt, locale)}</td>
          </tr>)}
        </Table>}
        {nextCursor ? <div className="request-more"><Button type="button" variant="ghost" loading={paging} onClick={loadMore}>{t("Load more", "Показать ещё")}</Button></div> : null}
      </Card>
    </div>
  </>;
}
