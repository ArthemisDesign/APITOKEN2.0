"use client";

import Link from "next/link";
import { useParams } from "next/navigation";
import { useMemo, useState, type FormEvent } from "react";
import { Banner, CardGrid, LoadingGrid, PageHead, Pill, SectionHeader, StatCard } from "@/components/ui";
import { send } from "@/lib/api";
import { formatDate, nanoMoney } from "@/lib/format";
import { localeFor, useI18n } from "@/lib/i18n";
import { useResource } from "@/lib/resources";
import { toast } from "@/lib/toast";
import { parsePercentBps } from "../helpers";
import type { AdminPartner } from "../types";

type Draft = {
  commission: string;
  teamMaximum: string;
  teamInvitesEnabled: boolean;
  b2bEnabled: boolean;
  b2bMaximum: string;
  b2bCanDelegate: boolean;
  status: AdminPartner["status"];
  programEnabled: boolean;
};

function percent(value: number): string { return String(value / 100); }

function requestedEmail(value: string): string {
  try { return decodeURIComponent(value); } catch { return value; }
}

function statusLabel(status: AdminPartner["status"], t: (en: string, ru: string) => string): string {
  if (status === "active") return t("Active", "Активен");
  if (status === "suspended") return t("Suspended", "Приостановлен");
  return t("Pending", "Ожидает");
}

export default function PartnerDetailPage() {
  const params = useParams<{ id: string }>();
  const { lang, t } = useI18n();
  const locale = localeFor(lang);
  const email = requestedEmail(params.id).trim().toLowerCase();
  const { data, isLoading, refresh } = useResource<{ items: AdminPartner[] }>("/admin/referral/partners");
  const partner = useMemo(() => data?.items.find((item) => item.email?.toLowerCase() === email) ?? null, [data, email]);
  const baseDraft = useMemo<Draft | null>(() => partner ? ({
    commission: percent(partner.commissionBps),
    teamMaximum: percent(partner.teamOverrideMaxBps),
    teamInvitesEnabled: partner.teamInvitesEnabled,
    b2bEnabled: partner.b2bEnabled,
    b2bMaximum: percent(partner.b2bMaxDiscountBps),
    b2bCanDelegate: partner.b2bCanDelegate,
    status: partner.status,
    programEnabled: partner.programEnabled,
  }) : null, [partner]);
  const [draftState, setDraftState] = useState<{ email: string; value: Draft } | null>(null);
  const draft = partner && draftState?.email === partner.email ? draftState.value : baseDraft;
  const setDraft = (value: Draft) => { if (partner?.email) setDraftState({ email: partner.email, value }); };
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  function fail(message: string, fieldId: string) {
    setError(message);
    window.requestAnimationFrame(() => document.getElementById(fieldId)?.focus());
  }

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!draft || !partner?.email) return;
    setError(null);
    const commissionBps = parsePercentBps(draft.commission, 10_000);
    const teamOverrideMaxBps = parsePercentBps(draft.teamMaximum, 2_000);
    const b2bMaxDiscountBps = parsePercentBps(draft.b2bMaximum, 9_500);
    if (commissionBps === null || teamOverrideMaxBps === null || b2bMaxDiscountBps === null) {
      fail(t("Check the percentages: commission ≤ 100%, Team share ≤ 20%, B2B discount ≤ 95%.", "Проверьте проценты: комиссия ≤ 100%, Team-доля ≤ 20%, B2B-скидка ≤ 95%."), commissionBps === null ? "partner-direct-commission" : teamOverrideMaxBps === null ? "partner-team-maximum" : "partner-b2b-maximum");
      return;
    }
    setBusy(true);
    try {
      await send("/admin/referral/partners", "PATCH", {
        email: partner.email,
        commissionBps,
        teamOverrideMaxBps,
        teamInvitesEnabled: true,
        b2bEnabled: b2bMaxDiscountBps > 0,
        b2bMaxDiscountBps,
        b2bCanDelegate: b2bMaxDiscountBps > 0,
        status: draft.status,
        programEnabled: draft.programEnabled,
      });
      refresh();
      setDraftState(null);
      toast(t("Partner settings saved", "Настройки партнёра сохранены"));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("Could not save settings.", "Не удалось сохранить настройки."));
    } finally {
      setBusy(false);
    }
  }

  if (isLoading && !data) return <><PageHead title={t("Partner", "Партнёр")} sub={t("Loading account", "Загружаем аккаунт")} /><LoadingGrid label={t("Loading Partner Account", "Загрузка аккаунта партнёра")} /></>;
  if (!partner) return <Banner kind="bad" title={t("Partner Not Found", "Партнёр не найден")}><Link className="link" href="/partners/directory">{t("Back to Partner Directory", "Вернуться к списку")}</Link></Banner>;

  return <>
    <PageHead title={<span translate="no">{partner.email}</span>} sub={<>{t("Commerce account", "Commerce-аккаунт")}{partner.parentEmail ? <> · {t("Team of", "Команда")} <span translate="no">{partner.parentEmail}</span></> : null} · <span className="mono" translate="no">{partner.referralCode}</span></>} badge={<Pill kind={partner.status === "active" && partner.programEnabled ? "ok" : partner.status === "suspended" ? "bad" : "warn"}>{partner.programEnabled ? statusLabel(partner.status, t) : t("Program Disabled", "Программа отключена")}</Pill>} />
    <div aria-live="polite">{error ? <Banner kind="bad" title={t("Action Failed", "Действие не выполнено")}>{error}</Banner> : null}</div>
    <div className="partner-detail-actions"><Link className="btn ghost" href="/partners/directory">← {t("Partner Directory", "К списку")}</Link></div>

    <CardGrid>
      <StatCard label={t("Direct Commission", "Прямая комиссия")} value={formatBps(partner.commissionBps)} hint={t("platform-funded", "выплачивает платформа")} />
      <StatCard label={t("Retained Team Share Max", "Макс. удерживаемая Team-доля")} value={formatBps(partner.teamOverrideMaxBps)} hint={t("hard platform cap 20%", "глобальный предел 20%")} />
      <StatCard label={t("Current Parent Share", "Текущая доля родителя")} value={partner.teamShareBps === null ? "—" : formatBps(partner.teamShareBps)} hint={partner.parentEmail ?? t("root partner", "корневой партнёр")} />
      <StatCard label={t("Referrals", "Рефералы")} value={partner.referredUsers} hint={`${partner.teamSize} ${t("Team members", "участников Team")}`} />
      <StatCard label={t("Net Earnings", "Чистый заработок")} value={nanoMoney(partner.netNano)} hint={`${nanoMoney(partner.adjustmentNano)} ${t("adjustments", "корректировки")}`} />
      <StatCard label={t("Payable", "К выплате")} value={nanoMoney(partner.payableNano)} hint={`${nanoMoney(partner.debtNano)} ${t("debt", "долг")}`} />
    </CardGrid>

    {draft ? <>
      <SectionHeader title={t("Authority & Terms", "Права и условия")} sub={t("Suspension preserves attribution and financial history", "Приостановка сохраняет атрибуцию и финансовую историю")} />
      <form className="partner-authority-form form-card" onSubmit={save} noValidate>
        <label className="field"><span>{t("Direct Commission", "Прямая комиссия")}</span><div className="percent-input"><input id="partner-direct-commission" name="commissionPercent" type="number" inputMode="decimal" autoComplete="off" min="0" max="100" step="0.01" value={draft.commission} onChange={(event) => setDraft({ ...draft, commission: event.target.value })} disabled={busy} /><i>%</i></div><small>{t("Set only by the platform", "Устанавливает только платформа")}</small></label>
        <label className="field"><span>{t("Maximum Retained Team Share", "Максимальная удерживаемая Team-доля")}</span><div className="percent-input"><input id="partner-team-maximum" name="teamShareMaximumPercent" type="number" inputMode="decimal" autoComplete="off" min="0" max="20" step="0.01" value={draft.teamMaximum} onChange={(event) => setDraft({ ...draft, teamMaximum: event.target.value })} disabled={busy} /><i>%</i></div><small>{t("The partner chooses a smaller share for each direct Team member", "Партнёр выбирает меньшую долю для каждого прямого участника Team")}</small></label>
        <label className="field"><span>{t("Partner Status", "Статус партнёра")}</span><select name="partnerStatus" value={draft.status} onChange={(event) => setDraft({ ...draft, status: event.target.value as Draft["status"] })} disabled={busy}><option value="active">{t("Active", "Активен")}</option><option value="suspended">{t("Suspended", "Приостановлен")}</option><option value="pending">{t("Pending", "Ожидает")}</option></select></label>
        <label className="field"><span>{t("Created", "Создан")}</span><input name="partnerCreatedAt" autoComplete="off" value={formatDate(partner.createdAt, true, locale)} readOnly aria-label={t("Partner Created At", "Дата создания партнёра")} /></label>
        <label className="field"><span>{t("Maximum Customer Discount", "Максимальная скидка клиенту")}</span><div className="percent-input"><input id="partner-b2b-maximum" name="b2bMaximumPercent" type="number" inputMode="decimal" autoComplete="off" min="0" max="95" step="1" value={draft.b2bMaximum} onChange={(event) => setDraft({ ...draft, b2bMaximum: event.target.value })} disabled={busy} /><i>%</i></div><small>{t("The ceiling for the partner's own B2B terms; 0% switches B2B off for them", "Потолок собственных B2B-условий партнёра; 0% выключает B2B для него")}</small></label>
        <fieldset className="partner-permissions"><legend>{t("Access", "Доступ")}</legend>
          <label className="admin-check"><input name="programEnabled" type="checkbox" checked={draft.programEnabled} onChange={(event) => setDraft({ ...draft, programEnabled: event.target.checked })} disabled={busy} /><span><b>{t("Partner Program Enabled", "Партнёрская программа включена")}</b><small>{t("Disable access without deleting history", "Отключает доступ без удаления истории")}</small></span></label>
        </fieldset>
        <div className="partner-authority-actions"><span>{t("No account deletion: financial history remains auditable.", "Удаления аккаунта нет: финансовая история остаётся доступной для аудита.")}</span><button type="submit" className="btn" disabled={busy}>{busy ? t("Saving…", "Сохраняем…") : t("Save Partner Settings", "Сохранить настройки")}</button></div>
      </form>
    </> : null}
  </>;
}

function formatBps(value: number): string {
  const pct = value / 100;
  return `${Number.isInteger(pct) ? pct : pct.toFixed(2).replace(/0+$/, "").replace(/\.$/, "")}%`;
}
