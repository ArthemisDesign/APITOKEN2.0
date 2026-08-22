"use client";

import { useState, type FormEvent } from "react";
import { Banner, EmptyRow, LoadingGrid, Modal, PageHead, Pill, SectionHeader, TableCard } from "@/components/ui";
import { send } from "@/lib/api";
import { formatDate } from "@/lib/format";
import { localeFor, useI18n } from "@/lib/i18n";
import { useResource } from "@/lib/resources";
import { toast } from "@/lib/toast";
import { parsePercentBps } from "../helpers";
import type { PartnerApplication, RootInvite } from "../types";

type Terms = {
  commission: string;
  defaultOverride: string;
  teamMaximum: string;
  teamInvitesEnabled: boolean;
  b2bEnabled: boolean;
  b2bMaximum: string;
  b2bCanDelegate: boolean;
};
type ApplicationDecision = Terms & { application: PartnerApplication; action: "approve" | "reject"; note: string };

const DEFAULT_TERMS: Terms = {
  commission: "10",
  defaultOverride: "10",
  teamMaximum: "20",
  teamInvitesEnabled: true,
  b2bEnabled: false,
  b2bMaximum: "0",
  b2bCanDelegate: false,
};

function formatBps(value: number | null, empty = "—"): string {
  if (value === null) return empty;
  const pct = value / 100;
  return `${Number.isInteger(pct) ? pct : pct.toFixed(2).replace(/0+$/, "").replace(/\.$/, "")}%`;
}

export default function PartnerOnboardingPage() {
  const { lang, t } = useI18n();
  const locale = localeFor(lang);
  const { data: applicationData, isLoading: applicationsLoading, refresh: refreshApplications } = useResource<{ items: PartnerApplication[] }>("/partner-admin/applications");
  const { data: inviteData, isLoading: invitesLoading, refresh: refreshInvites } = useResource<{ items: RootInvite[] }>("/partner-admin/invites");
  const [telegram, setTelegram] = useState("");
  const [terms, setTerms] = useState<Terms>(DEFAULT_TERMS);
  const [created, setCreated] = useState<RootInvite | null>(null);
  const [decision, setDecision] = useState<ApplicationDecision | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  function showError(message: string, fieldId?: string) {
    setError(message);
    if (fieldId) window.requestAnimationFrame(() => document.getElementById(fieldId)?.focus());
  }

  async function copyInvite(invite: RootInvite) {
    setError(null);
    try {
      await navigator.clipboard.writeText(invite.inviteUrl);
      toast(t("Link copied", "Ссылка скопирована"));
    } catch {
      setCreated(invite);
      setError(t(
        "Could not copy the link. It is now shown in the invite field above for manual copying.",
        "Не удалось скопировать ссылку. Теперь она показана в поле приглашения выше — скопируйте её вручную.",
      ));
      window.requestAnimationFrame(() => document.querySelector<HTMLInputElement>('input[name="createdInviteUrl"]')?.focus());
    }
  }

  function validateTerms(value: Terms, idPrefix: string): { commissionBps: number; subCommissionBps: number; teamOverrideMaxBps: number; b2bMaxDiscountBps: number } | null {
    const commissionBps = parsePercentBps(value.commission, 10_000);
    const subCommissionBps = parsePercentBps(value.defaultOverride, 2_000);
    const teamOverrideMaxBps = parsePercentBps(value.teamMaximum, 2_000);
    const b2bMaxDiscountBps = value.b2bEnabled ? parsePercentBps(value.b2bMaximum, 9_500) : 0;
    if (commissionBps === null || subCommissionBps === null || teamOverrideMaxBps === null || b2bMaxDiscountBps === null || (value.b2bEnabled && b2bMaxDiscountBps <= 0)) {
      const fieldId = commissionBps === null
        ? `${idPrefix}-commission`
        : subCommissionBps === null
          ? `${idPrefix}-default-override`
          : teamOverrideMaxBps === null
            ? `${idPrefix}-team-maximum`
            : `${idPrefix}-b2b-maximum`;
      showError(t("Check the percentages: direct ≤100%, Team ≤20%, B2B 1–95% when enabled.", "Проверьте проценты: прямая ≤100%, Team ≤20%, B2B 1–95% при включении."), fieldId);
      return null;
    }
    return { commissionBps, subCommissionBps, teamOverrideMaxBps, b2bMaxDiscountBps };
  }

  async function createInvite(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setError(null); setCreated(null);
    const username = telegram.trim().replace(/^@/, "");
    if (!/^[A-Za-z0-9_]{5,32}$/.test(username)) return showError(t("Enter a valid Telegram username.", "Введите корректное имя пользователя Telegram."), "root-invite-telegram");
    const parsed = validateTerms(terms, "root-invite");
    if (!parsed) return;
    setBusy(true);
    try {
      const invite = await send<RootInvite>("/partner-admin/invites", "POST", {
        telegramUsername: username,
        ...parsed,
        teamInvitesEnabled: terms.teamInvitesEnabled,
        b2bEnabled: terms.b2bEnabled,
        b2bMaxDiscountBps: parsed.b2bMaxDiscountBps,
        b2bCanDelegate: terms.b2bEnabled && terms.b2bCanDelegate,
      });
      setCreated(invite); setTelegram(""); refreshInvites();
      toast(t("Invite created", "Приглашение создано"));
    } catch (cause) { setError(cause instanceof Error ? cause.message : t("Could not create invite.", "Не удалось создать приглашение.")); }
    finally { setBusy(false); }
  }

  async function decideApplication(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!decision) return;
    setError(null);
    const note = decision.note.trim();
    if (!note) return showError(t("A decision note is required.", "Комментарий к решению обязателен."), "application-decision-note");
    const parsed = decision.action === "approve" ? validateTerms(decision, "application-decision") : null;
    if (decision.action === "approve" && !parsed) return;
    setBusy(true);
    try {
      await send(`/partner-admin/applications/${decision.application.id}/decision`, "POST", {
        action: decision.action,
        note,
        ...(parsed ? {
          commissionBps: parsed.commissionBps,
          subCommissionBps: parsed.subCommissionBps,
          teamOverrideMaxBps: parsed.teamOverrideMaxBps,
          teamInvitesEnabled: decision.teamInvitesEnabled,
          b2bEnabled: decision.b2bEnabled,
          b2bMaxDiscountBps: parsed.b2bMaxDiscountBps,
          b2bCanDelegate: decision.b2bEnabled && decision.b2bCanDelegate,
        } : {}),
      });
      toast(decision.action === "approve" ? t("Application approved", "Заявка одобрена") : t("Application rejected", "Заявка отклонена"));
      setDecision(null); refreshApplications();
    } catch (cause) { setError(cause instanceof Error ? cause.message : t("Decision failed.", "Не удалось сохранить решение.")); }
    finally { setBusy(false); }
  }

  const applications = applicationData?.items ?? [];
  const pending = applications.filter((item) => item.status === "pending");
  const invites = inviteData?.items ?? [];
  if ((applicationsLoading && !applicationData) || (invitesLoading && !inviteData)) return <><PageHead title={t("Partner onboarding", "Подключение партнёров")} /><LoadingGrid label={t("Loading partner onboarding", "Загрузка онбординга партнёров")} /></>;

  return <>
    <PageHead title={t("Partner onboarding", "Подключение партнёров")} sub={t("Set every authority boundary before the account starts operating", "Задайте все границы полномочий до начала работы аккаунта")} badge={<Pill kind={pending.length ? "warn" : "ok"}>{pending.length} {t("applications", "заявок")}</Pill>} />
    <div aria-live="polite">{error && !decision ? <div role="alert"><Banner kind="bad" title={t("Onboarding action failed", "Ошибка онбординга")}>{error}</Banner></div> : null}</div>

    <SectionHeader title={t("Create a root invite", "Создать корневое приглашение")} sub={t("Telegram is used only to bind the pre-account invite; after registration email is the displayed identity", "Telegram нужен только для привязки приглашения до создания аккаунта; после регистрации отображается email")} />
    <form className="partner-onboarding-form form-card" onSubmit={createInvite} noValidate>
      <label className="field partner-onboarding-telegram"><span>{t("Telegram username", "Имя пользователя Telegram")}</span><input id="root-invite-telegram" name="telegramUsername" value={telegram} onChange={(event) => setTelegram(event.target.value)} placeholder="@partner_name…" autoComplete="off" spellCheck={false} disabled={busy} /></label>
      <TermsEditor idPrefix="root-invite" value={terms} onChange={setTerms} disabled={busy} t={t} />
      <div className="partner-authority-actions"><button className="btn" type="submit" disabled={busy}>{busy ? t("Creating…", "Создаём…") : t("Create invite", "Создать приглашение")}</button></div>
      {created ? <div className="created-root-invite"><label className="field"><span>{t("Invite link", "Ссылка приглашения")}</span><input name="createdInviteUrl" autoComplete="off" readOnly translate="no" value={created.inviteUrl} onFocus={(event) => event.currentTarget.select()} /></label><button className="btn ghost" type="button" onClick={() => copyInvite(created)}>{t("Copy", "Копировать")}</button></div> : null}
    </form>

    <SectionHeader title={t("Open applications", "Входящие заявки")} sub={t("Applicants entered through Telegram without an invite", "Партнёры, подавшие заявку через Telegram без приглашения")} />
    <TableCard><table><thead><tr><th className="left">{t("Applicant", "Кандидат")}</th><th className="left">{t("Reason", "Обоснование")}</th><th>{t("Created", "Создана")}</th><th>{t("Status", "Статус")}</th><th><span className="sr-only">{t("Actions", "Действия")}</span></th></tr></thead><tbody>
      {applications.length ? applications.map((application) => <tr key={application.id}><td className="left"><b translate="no">{application.displayName ?? (application.telegramUsername ? `@${application.telegramUsername}` : application.id.slice(0, 8))}</b>{application.displayName && application.telegramUsername ? <div className="sub" translate="no">@{application.telegramUsername}</div> : null}</td><td className="left"><div className="partner-request-reason" title={application.note ?? undefined}>{application.note ?? "—"}</div>{application.adminNote ? <div className="sub partner-request-note">{application.adminNote}</div> : null}</td><td>{formatDate(application.createdAt, true, locale)}</td><td><Pill kind={application.status === "approved" ? "ok" : application.status === "rejected" ? "bad" : "warn"}>{application.status === "approved" ? t("Approved", "Одобрена") : application.status === "rejected" ? t("Rejected", "Отклонена") : t("Pending", "Ожидает")}</Pill></td><td>{application.status === "pending" ? <div className="actions"><button className="btn" type="button" onClick={() => { setError(null); setDecision({ application, action: "approve", note: "", ...DEFAULT_TERMS }); }}>{t("Review", "Рассмотреть")}</button><button className="btn bad" type="button" onClick={() => { setError(null); setDecision({ application, action: "reject", note: "", ...DEFAULT_TERMS }); }}>{t("Reject", "Отклонить")}</button></div> : "—"}</td></tr>) : <EmptyRow columns={5} text={t("No applications", "Заявок нет")} />}
    </tbody></table></TableCard>

    <SectionHeader title={t("Root invitations", "Корневые приглашения")} sub={t("One-time links expire after 30 days", "Одноразовые ссылки действуют 30 дней")} />
    <TableCard><table><thead><tr><th className="left">{t("Pre-account identity", "Идентификатор до регистрации")}</th><th>{t("Direct", "Прямая")}</th><th>{t("Team maximum", "Макс. Team")}</th><th>{t("Capabilities", "Возможности")}</th><th>{t("Expires", "Истекает")}</th><th>{t("Status", "Статус")}</th><th><span className="sr-only">{t("Link", "Ссылка")}</span></th></tr></thead><tbody>
      {invites.length ? invites.map((invite) => <tr key={invite.code}><td className="left mono" translate="no">{invite.telegramUsername ? `@${invite.telegramUsername}` : "—"}</td><td>{formatBps(invite.commissionBps, t("Default", "По умолчанию"))}</td><td>{formatBps(invite.teamOverrideMaxBps)}</td><td><div className="permission-stack"><span>{invite.teamInvitesEnabled ? t("Team enabled", "Team включён") : t("Team disabled", "Team отключён")}</span><small>{invite.b2bEnabled ? `B2B ≤ ${formatBps(invite.b2bMaxDiscountBps)}${invite.b2bCanDelegate ? " ↗" : ""}` : t("B2B disabled", "B2B отключён")}</small></div></td><td>{formatDate(invite.expiresAt, false, locale)}</td><td><Pill kind={invite.consumedAt ? "ok" : "warn"}>{invite.consumedAt ? t("Used", "Использовано") : t("Waiting", "Ожидает")}</Pill></td><td>{invite.consumedAt ? "—" : <button className="btn ghost" type="button" onClick={() => copyInvite(invite)}>{t("Copy", "Копировать")}</button>}</td></tr>) : <EmptyRow columns={7} text={t("No root invites", "Корневых приглашений нет")} />}
    </tbody></table></TableCard>

    <Modal open={Boolean(decision)} wide title={decision?.action === "approve" ? t("Approve application", "Одобрить заявку") : t("Reject application", "Отклонить заявку")} message={decision ? `${decision.application.displayName ?? ""} ${decision.application.telegramUsername ? `@${decision.application.telegramUsername}` : ""}` : undefined} onClose={() => { if (!busy) { setDecision(null); setError(null); } }}>
      {decision ? <form className="partner-onboarding-form" onSubmit={decideApplication} noValidate>
        {error ? <div className="partner-form-error" role="alert" aria-live="polite">{error}</div> : null}
        {decision.action === "approve" ? <TermsEditor idPrefix="application-decision" value={decision} onChange={(value) => setDecision({ ...decision, ...value })} disabled={busy} t={t} /> : null}
        <label className="field"><span>{t("Decision note", "Комментарий к решению")}</span><textarea id="application-decision-note" name="decisionNote" autoComplete="off" rows={4} maxLength={2000} value={decision.note} onChange={(event) => setDecision({ ...decision, note: event.target.value })} disabled={busy} placeholder={t("Reason and expected commercial effect…", "Причина и ожидаемый коммерческий эффект…")} /></label>
        <div className="dlg-actions"><button type="button" className="btn ghost" onClick={() => { setDecision(null); setError(null); }} disabled={busy}>{t("Cancel", "Отмена")}</button><button type="submit" className={`btn${decision.action === "reject" ? " bad" : ""}`} disabled={busy}>{busy ? t("Saving…", "Сохраняем…") : decision.action === "approve" ? t("Approve and configure", "Одобрить и настроить") : t("Reject", "Отклонить")}</button></div>
      </form> : null}
    </Modal>
  </>;
}

function TermsEditor({ idPrefix, value, onChange, disabled, t }: { idPrefix: string; value: Terms; onChange: (value: Terms) => void; disabled: boolean; t: (en: string, ru: string) => string }) {
  return <div className="partner-terms-grid">
    <label className="field"><span>{t("Direct commission", "Прямая комиссия")}</span><div className="percent-input"><input id={`${idPrefix}-commission`} name="commissionPercent" type="number" inputMode="decimal" autoComplete="off" min="0" max="100" step="0.01" value={value.commission} onChange={(event) => onChange({ ...value, commission: event.target.value })} disabled={disabled} /><i>%</i></div></label>
    <label className="field"><span>{t("Default Team override", "Надбавка Team по умолчанию")}</span><div className="percent-input"><input id={`${idPrefix}-default-override`} name="defaultTeamOverridePercent" type="number" inputMode="decimal" autoComplete="off" min="0" max="20" step="0.01" value={value.defaultOverride} onChange={(event) => onChange({ ...value, defaultOverride: event.target.value })} disabled={disabled} /><i>%</i></div></label>
    <label className="field"><span>{t("Maximum Team override", "Максимальная надбавка Team")}</span><div className="percent-input"><input id={`${idPrefix}-team-maximum`} name="teamOverrideMaximumPercent" type="number" inputMode="decimal" autoComplete="off" min="0" max="20" step="0.01" value={value.teamMaximum} onChange={(event) => onChange({ ...value, teamMaximum: event.target.value })} disabled={disabled} /><i>%</i></div></label>
    <label className="admin-check"><input name={`${idPrefix}-teamInvitesEnabled`} type="checkbox" checked={value.teamInvitesEnabled} onChange={(event) => onChange({ ...value, teamInvitesEnabled: event.target.checked })} disabled={disabled} /><span><b>{t("May invite a team", "Может приглашать Team")}</b><small>{t("Creates direct partner invitations", "Создаёт приглашения прямых участников")}</small></span></label>
    <label className="admin-check"><input name={`${idPrefix}-b2bEnabled`} type="checkbox" checked={value.b2bEnabled} onChange={(event) => onChange({ ...value, b2bEnabled: event.target.checked, b2bCanDelegate: event.target.checked && value.b2bCanDelegate })} disabled={disabled} /><span><b>{t("B2B self-service", "Самостоятельный B2B")}</b><small>{t("Can convert owned referrals", "Может переводить своих рефералов")}</small></span></label>
    {value.b2bEnabled ? <><label className="field"><span>{t("Maximum B2B discount", "Максимальная B2B-скидка")}</span><div className="percent-input"><input id={`${idPrefix}-b2b-maximum`} name="b2bMaximumPercent" type="number" inputMode="decimal" autoComplete="off" min="1" max="95" step="0.01" value={value.b2bMaximum} onChange={(event) => onChange({ ...value, b2bMaximum: event.target.value })} disabled={disabled} /><i>%</i></div></label><label className="admin-check"><input name={`${idPrefix}-b2bCanDelegate`} type="checkbox" checked={value.b2bCanDelegate} onChange={(event) => onChange({ ...value, b2bCanDelegate: event.target.checked })} disabled={disabled} /><span><b>{t("May delegate B2B", "Может делегировать B2B")}</b><small>{t("Only within their own ceiling", "Только в пределах своего лимита")}</small></span></label></> : null}
  </div>;
}
