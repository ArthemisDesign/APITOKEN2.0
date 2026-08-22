"use client";

import { useMemo, useState, type FormEvent } from "react";
import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { Banner, CardGrid, EmptyRow, LoadingGrid, PageHead, Pill, SectionHeader, StatCard, TableCard } from "@/components/ui";
import { send } from "@/lib/api";
import { dialog } from "@/lib/dialog";
import { ago, formatDate, nanoMoney } from "@/lib/format";
import { localeFor, useI18n } from "@/lib/i18n";
import { useResource } from "@/lib/resources";
import { toast } from "@/lib/toast";
import { parsePercentBps } from "../helpers";
import type { AdminPartner, PartnerActivity, PartnerDetailBundle } from "../types";

type Draft = {
  commission: string;
  defaultOverride: string;
  teamMaximum: string;
  teamInvitesEnabled: boolean;
  b2bEnabled: boolean;
  b2bMaximum: string;
  b2bCanDelegate: boolean;
  status: AdminPartner["status"];
};

function percent(value: number): string { return String(value / 100); }
function identity(partner: AdminPartner): string {
  return partner.email ?? partner.displayName ?? (partner.telegramUsername ? `@${partner.telegramUsername}` : partner.id);
}
function statusLabel(status: AdminPartner["status"], t: (en: string, ru: string) => string): string {
  if (status === "active") return t("Active", "Активен");
  if (status === "suspended") return t("Suspended", "Приостановлен");
  return t("Pending", "Ожидает");
}

export default function PartnerDetailPage() {
  const params = useParams<{ id: string }>();
  const router = useRouter();
  const { lang, t } = useI18n();
  const locale = localeFor(lang);
  const id = params.id;
  const { data: partners, isLoading: partnersLoading, refresh: refreshPartners } = useResource<{ items: AdminPartner[] }>("/partner-admin/partners");
  const { data: detail, isLoading: detailLoading, refresh: refreshDetail } = useResource<PartnerDetailBundle>(`/partner-admin/partners/${id}/analytics`);
  const { data: activity } = useResource<{ events: PartnerActivity[] }>(`/partner-admin/partners/${id}/activity?limit=80`);
  const partner = useMemo(() => partners?.items.find((item) => item.id === id) ?? null, [id, partners]);
  const baseDraft = useMemo<Draft | null>(() => partner ? ({
    commission: percent(partner.commissionBps),
    defaultOverride: percent(partner.subCommissionBps),
    teamMaximum: percent(partner.teamOverrideMaxBps),
    teamInvitesEnabled: partner.teamInvitesEnabled,
    b2bEnabled: partner.b2bEnabled,
    b2bMaximum: percent(partner.b2bMaxDiscountBps),
    b2bCanDelegate: partner.b2bCanDelegate,
    status: partner.status,
  }) : null, [partner]);
  const [draftState, setDraftState] = useState<{ partnerId: string; value: Draft } | null>(null);
  const draft = partner && draftState?.partnerId === partner.id ? draftState.value : baseDraft;
  const setDraft = (value: Draft) => {
    if (partner) setDraftState({ partnerId: partner.id, value });
  };
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  function showError(message: string, fieldId?: string) {
    setError(message);
    if (fieldId) window.requestAnimationFrame(() => document.getElementById(fieldId)?.focus());
  }

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!draft || !partner) return;
    setError(null);
    const commissionBps = parsePercentBps(draft.commission, 10_000);
    const subCommissionBps = parsePercentBps(draft.defaultOverride, 2_000);
    const teamOverrideMaxBps = parsePercentBps(draft.teamMaximum, 2_000);
    const b2bMaxDiscountBps = draft.b2bEnabled ? parsePercentBps(draft.b2bMaximum, 9_500) : 0;
    if (commissionBps === null || subCommissionBps === null || teamOverrideMaxBps === null || b2bMaxDiscountBps === null || (draft.b2bEnabled && b2bMaxDiscountBps <= 0)) {
      const fieldId = commissionBps === null
        ? "partner-direct-commission"
        : subCommissionBps === null
          ? "partner-default-override"
          : teamOverrideMaxBps === null
            ? "partner-team-maximum"
            : "partner-b2b-maximum";
      showError(t(
        "Check the percentages: direct ≤ 100%, Team values ≤ 20%, B2B is 1–95% when enabled.",
        "Проверьте проценты: прямая ≤ 100%, значения Team ≤ 20%, B2B — 1–95% при включении.",
      ), fieldId);
      return;
    }
    setBusy(true);
    try {
      await send(`/partner-admin/partners/${id}`, "PATCH", {
        commissionBps,
        subCommissionBps,
        teamOverrideMaxBps,
        teamInvitesEnabled: draft.teamInvitesEnabled,
        b2bEnabled: draft.b2bEnabled,
        b2bMaxDiscountBps,
        b2bCanDelegate: draft.b2bEnabled && draft.b2bCanDelegate,
        status: draft.status,
      });
      refreshPartners();
      refreshDetail();
      toast(t("Partner settings saved", "Настройки партнёра сохранены"));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("Could not save settings.", "Не удалось сохранить настройки."));
    } finally { setBusy(false); }
  }

  async function remove() {
    if (!partner) return;
    const confirmation = await dialog({
      title: t("Delete partner", "Удалить партнёра"),
      message: t(
        `Delete ${identity(partner)} only if the account has no history. Otherwise suspend it. Type DELETE to confirm.`,
        `Удаление ${identity(partner)} возможно только без истории. Иначе приостановите аккаунт. Введите DELETE для подтверждения.`,
      ),
      fields: [{ name: "confirm", label: "DELETE" }],
      confirmLabel: t("Delete permanently", "Удалить безвозвратно"),
      danger: true,
    });
    if (confirmation?.confirm !== "DELETE") return;
    setBusy(true);
    setError(null);
    try {
      await send(`/partner-admin/partners/${id}`, "DELETE", {});
      toast(t("Partner deleted", "Партнёр удалён"));
      router.replace("/partners/directory");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("Could not delete partner.", "Не удалось удалить партнёра."));
      setBusy(false);
    }
  }

  if ((partnersLoading && !partners) || (detailLoading && !detail)) return <><PageHead title={t("Partner", "Партнёр")} sub={t("Loading account", "Загружаем аккаунт")} /><LoadingGrid label={t("Loading partner account", "Загрузка аккаунта партнёра")} /></>;
  if (!partner) return <Banner kind="bad" title={t("Partner not found", "Партнёр не найден")}><Link className="link" href="/partners/directory">{t("Back to directory", "Вернуться к списку")}</Link></Banner>;

  const p = detail?.partner;
  const events = (activity?.events ?? []).filter((event) => !event.type.startsWith("promo") && !event.type.startsWith("discount_link"));

  return <>
    <PageHead title={<span translate="no">{identity(partner)}</span>} sub={<>{partner.displayName ?? t("Partner account", "Партнёрский аккаунт")} · <span className="mono" translate="no">{partner.referralCode}</span>{partner.parentEmail ? <> · {t("team of", "команда")} <span translate="no">{partner.parentEmail}</span></> : null}</>} badge={<Pill kind={partner.status === "active" ? "ok" : partner.status === "suspended" ? "bad" : "warn"}>{statusLabel(partner.status, t)}</Pill>} />
    <div aria-live="polite">{error ? <Banner kind="bad" title={t("Action failed", "Действие не выполнено")}>{error}</Banner> : null}</div>
    <div className="partner-detail-actions"><Link className="btn ghost" href="/partners/directory">← {t("Directory", "К списку")}</Link></div>

    <CardGrid>
      <StatCard label={t("Direct commission", "Прямая комиссия")} value={formatBps(partner.commissionBps)} hint={t("platform-funded", "выплачивает платформа")} />
      <StatCard label={t("Team override maximum", "Макс. надбавка Team")} value={formatBps(partner.teamOverrideMaxBps)} hint={t("hard platform cap 20%", "глобальный предел 20%")} />
      <StatCard label={t("Referrals", "Рефералы")} value={p?.referredUsers ?? partner.referredUsers} hint={`${p?.convertedUsers ?? "—"} ${t("paying", "платящих")}`} />
      <StatCard label={t("Team", "Команда")} value={p?.teamSize ?? partner.teamSize} hint={partner.teamInvitesEnabled ? t("invitations enabled", "приглашения включены") : t("invitations disabled", "приглашения отключены")} />
      <StatCard label={t("Net earnings", "Чистый заработок")} value={nanoMoney(p?.netTotalNano ?? partner.netNano)} hint={`${nanoMoney(p?.net30dNano)} · 30d`} />
      <StatCard label={t("Payable", "К выплате")} value={nanoMoney(p?.payableNano ?? partner.payableNano)} hint={`${nanoMoney(p?.debtNano ?? partner.debtNano)} ${t("debt", "долг")}`} />
    </CardGrid>

    {draft ? <>
      <SectionHeader title={t("Authority and terms", "Права и условия")} sub={t("All changes are audited with the authenticated administrator", "Все изменения записываются вместе с аккаунтом вошедшего администратора")} />
      <form className="partner-authority-form form-card" onSubmit={save} noValidate>
        <label className="field"><span>{t("Direct commission", "Прямая комиссия")}</span><div className="percent-input"><input id="partner-direct-commission" name="commissionPercent" type="number" inputMode="decimal" autoComplete="off" min="0" max="100" step="0.01" value={draft.commission} onChange={(event) => setDraft({ ...draft, commission: event.target.value })} disabled={busy} /><i>%</i></div><small>{t("Share of owned referrals' real paid spend", "Доля от реального оплаченного расхода своих рефералов")}</small></label>
        <label className="field"><span>{t("Default Team override", "Надбавка Team по умолчанию")}</span><div className="percent-input"><input id="partner-default-override" name="defaultTeamOverridePercent" type="number" inputMode="decimal" autoComplete="off" min="0" max="20" step="0.01" value={draft.defaultOverride} onChange={(event) => setDraft({ ...draft, defaultOverride: event.target.value })} disabled={busy} /><i>%</i></div><small>{t("Suggested edge value; each member is configurable", "Стартовое значение ребра; каждый участник настраивается отдельно")}</small></label>
        <label className="field"><span>{t("Maximum Team override", "Максимальная надбавка Team")}</span><div className="percent-input"><input id="partner-team-maximum" name="teamOverrideMaximumPercent" type="number" inputMode="decimal" autoComplete="off" min="0" max="20" step="0.01" value={draft.teamMaximum} onChange={(event) => setDraft({ ...draft, teamMaximum: event.target.value })} disabled={busy} /><i>%</i></div><small>{t("Lowering it clamps the subtree leaf-first", "Снижение зажимает всё поддерево от листьев")}</small></label>
        <label className="field"><span>{t("Account status", "Статус аккаунта")}</span><select name="partnerStatus" value={draft.status} onChange={(event) => setDraft({ ...draft, status: event.target.value as Draft["status"] })} disabled={busy}><option value="active">{t("Active", "Активен")}</option><option value="suspended">{t("Suspended", "Приостановлен")}</option><option value="pending">{t("Pending", "Ожидает")}</option></select></label>
        <fieldset className="partner-permissions"><legend>{t("Delegated capabilities", "Делегируемые возможности")}</legend>
          <label className="admin-check"><input name="teamInvitesEnabled" type="checkbox" checked={draft.teamInvitesEnabled} onChange={(event) => setDraft({ ...draft, teamInvitesEnabled: event.target.checked })} disabled={busy} /><span><b>{t("Team invitations", "Приглашения Team")}</b><small>{t("Partner may recruit direct team members", "Партнёр может приглашать прямых участников")}</small></span></label>
          <label className="admin-check"><input name="b2bEnabled" type="checkbox" checked={draft.b2bEnabled} onChange={(event) => setDraft({ ...draft, b2bEnabled: event.target.checked, b2bCanDelegate: event.target.checked && draft.b2bCanDelegate })} disabled={busy} /><span><b>{t("B2B self-service", "Самостоятельный B2B")}</b><small>{t("May convert owned referrals without a request", "Может переводить своих рефералов без заявки")}</small></span></label>
          {draft.b2bEnabled ? <>
            <label className="field partner-b2b-limit"><span>{t("Maximum customer discount", "Максимальная скидка клиенту")}</span><div className="percent-input"><input id="partner-b2b-maximum" name="b2bMaximumPercent" type="number" inputMode="decimal" autoComplete="off" min="1" max="95" step="0.01" value={draft.b2bMaximum} onChange={(event) => setDraft({ ...draft, b2bMaximum: event.target.value })} disabled={busy} /><i>%</i></div></label>
            <label className="admin-check"><input name="b2bCanDelegate" type="checkbox" checked={draft.b2bCanDelegate} onChange={(event) => setDraft({ ...draft, b2bCanDelegate: event.target.checked })} disabled={busy} /><span><b>{t("May delegate B2B", "Может делегировать B2B")}</b><small>{t("May pass a smaller ceiling to team members", "Может передавать меньший лимит участникам Team")}</small></span></label>
          </> : null}
        </fieldset>
        <div className="partner-authority-actions"><button type="submit" className="btn" disabled={busy}>{busy ? t("Saving…", "Сохраняем…") : t("Save settings", "Сохранить настройки")}</button><button type="button" className="btn bad" disabled={busy} onClick={remove}>{t("Delete account", "Удалить аккаунт")}</button></div>
      </form>
    </> : null}

    <SectionHeader title={t("Team", "Команда")} sub={t("Email is the primary account identity", "Email — основной идентификатор аккаунта")} />
    <TableCard><table><thead><tr><th className="left">{t("Member", "Участник")}</th><th>{t("Direct rate", "Прямая ставка")}</th><th>{t("Referrals", "Рефералы")}</th><th>{t("Parent earnings", "Заработок родителя")}</th></tr></thead><tbody>
      {detail?.team.length ? detail.team.map((member) => <tr key={member.id}><td className="left"><b translate="no">{member.email ?? member.displayName ?? (member.telegramUsername ? `@${member.telegramUsername}` : member.id.slice(0, 8))}</b>{member.email && member.telegramUsername ? <div className="sub" translate="no">@{member.telegramUsername}</div> : null}</td><td>{formatBps(member.commissionBps)}</td><td>{member.referredUsers}</td><td>{nanoMoney(member.myOverrideNetNano)}</td></tr>) : <EmptyRow columns={4} text={t("No direct team members", "Нет прямых участников команды")} />}
    </tbody></table></TableCard>

    <SectionHeader title={t("Referred accounts", "Привлечённые аккаунты")} sub={t("Actual Commerce type and discount, never a legacy marker", "Фактический тип и скидка Commerce, без legacy-маркеров")} />
    <TableCard><table><thead><tr><th className="left">Email</th><th>{t("Type", "Тип")}</th><th>{t("Actual discount", "Фактическая скидка")}</th><th>{t("Spend", "Расход")}</th><th>{t("Partner earned", "Заработок партнёра")}</th><th>{t("Attributed", "Закреплён")}</th></tr></thead><tbody>
      {detail?.referrals.length ? detail.referrals.map((referral) => <tr key={`${referral.userRef}-${referral.attributedAt}`}><td className="left"><b className="mono" translate="no">{referral.email ?? referral.userMask}</b></td><td><Pill kind={referral.customerType === "b2b" ? "ok" : ""}>{referral.customerType?.toUpperCase() ?? "—"}</Pill></td><td>{referral.discountPercent == null ? "—" : `${referral.discountPercent}%`}</td><td>{nanoMoney(referral.spendNano)}</td><td>{nanoMoney(referral.netNano)}</td><td>{formatDate(referral.attributedAt, false, locale)}</td></tr>) : <EmptyRow columns={6} text={t("No referred accounts", "Нет привлечённых аккаунтов")} />}
    </tbody></table></TableCard>

    <SectionHeader title={t("Activity", "Активность")} sub={`${events.length}`} />
    <div className="partner-activity-list">{events.length ? events.map((event, index) => <div className="partner-activity-item" key={`${event.at}-${index}`}><span className="partner-activity-dot" aria-hidden /><div><b>{event.label}</b>{event.amountNano ? <span>{nanoMoney(event.amountNano)}</span> : null}</div><time dateTime={event.at} title={formatDate(event.at, true, locale)}>{ago(event.at, locale)}</time></div>) : <div className="empty">{t("No activity", "Активности нет")}</div>}</div>
  </>;
}

function formatBps(value: number): string {
  const pct = value / 100;
  return `${Number.isInteger(pct) ? pct : pct.toFixed(2).replace(/0+$/, "").replace(/\.$/, "")}%`;
}
