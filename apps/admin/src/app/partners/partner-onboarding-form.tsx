"use client";

import { useState, type FormEvent } from "react";
import { Banner, Modal } from "@/components/ui";
import { send } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { toast } from "@/lib/toast";
import { parsePercentBps } from "./helpers";

export type PartnerTermsDraft = {
  commission: string;
  teamMaximum: string;
  teamInvitesEnabled: boolean;
  b2bEnabled: boolean;
  b2bMaximum: string;
  b2bCanDelegate: boolean;
};

export type PartnerOnboardingPayload = {
  commissionBps: number;
  authority: {
    teamOverrideMaxBps: number;
    teamInvitesEnabled: boolean;
    b2bEnabled: boolean;
    b2bMaxDiscountBps: number;
    b2bCanDelegate: boolean;
  };
};

export const DEFAULT_PARTNER_TERMS: PartnerTermsDraft = {
  commission: "10",
  teamMaximum: "20",
  teamInvitesEnabled: true,
  b2bEnabled: false,
  b2bMaximum: "0",
  b2bCanDelegate: false,
};

export function partnerOnboardingPayload(value: PartnerTermsDraft): PartnerOnboardingPayload | null {
  const commissionBps = parsePercentBps(value.commission, 10_000);
  const teamOverrideMaxBps = parsePercentBps(value.teamMaximum, 2_000);
  const b2bMaxDiscountBps = value.b2bEnabled ? parsePercentBps(value.b2bMaximum, 9_500) : 0;
  if (commissionBps === null || teamOverrideMaxBps === null || b2bMaxDiscountBps === null) return null;
  return {
    commissionBps,
    authority: {
      teamOverrideMaxBps,
      teamInvitesEnabled: value.teamInvitesEnabled,
      b2bEnabled: value.b2bEnabled,
      b2bMaxDiscountBps,
      b2bCanDelegate: value.b2bEnabled && value.b2bCanDelegate,
    },
  };
}

export function PartnerTermsFields(props: {
  idPrefix: string;
  value: PartnerTermsDraft;
  onChange: (value: PartnerTermsDraft) => void;
  disabled: boolean;
}) {
  const { t } = useI18n();
  const { value, onChange } = props;
  return <>
    <div className="partner-terms-grid">
      <label className="field"><span>{t("Direct commission", "Прямая комиссия")}</span><div className="percent-input"><input id={`${props.idPrefix}-commission`} name="commissionPercent" type="number" inputMode="decimal" autoComplete="off" min="0" max="100" step="0.01" value={value.commission} onChange={(event) => onChange({ ...value, commission: event.target.value })} disabled={props.disabled} /><i>%</i></div><small>{t("Paid by the platform from real paid spend", "Платформа начисляет её с реального оплаченного расхода")}</small></label>
      <label className="field"><span>{t("Maximum retained Team share", "Максимальная удерживаемая Team-доля")}</span><div className="percent-input"><input id={`${props.idPrefix}-team-maximum`} name="teamShareMaximumPercent" type="number" inputMode="decimal" autoComplete="off" min="0" max="20" step="0.01" value={value.teamMaximum} onChange={(event) => onChange({ ...value, teamMaximum: event.target.value })} disabled={props.disabled} /><i>%</i></div><small>{t("A parent can retain less for each member; the platform hard cap is 20%", "Родитель задаёт меньше для каждого участника; предел платформы — 20%")}</small></label>
    </div>
    <fieldset className="partner-permissions">
      <legend>{t("Delegated capabilities", "Делегируемые возможности")}</legend>
      <label className="admin-check"><input name={`${props.idPrefix}-teamInvitesEnabled`} type="checkbox" checked={value.teamInvitesEnabled} onChange={(event) => onChange({ ...value, teamInvitesEnabled: event.target.checked })} disabled={props.disabled} /><span><b>{t("Team invitations", "Приглашения в Team")}</b><small>{t("The partner can invite existing Commerce accounts by email", "Партнёр может приглашать существующие Commerce-аккаунты по email")}</small></span></label>
      <label className="admin-check"><input name={`${props.idPrefix}-b2bEnabled`} type="checkbox" checked={value.b2bEnabled} onChange={(event) => onChange({ ...value, b2bEnabled: event.target.checked, b2bCanDelegate: event.target.checked && value.b2bCanDelegate })} disabled={props.disabled} /><span><b>{t("B2B self-service", "Самостоятельный перевод в B2B")}</b><small>{t("The partner can set pricing for owned referrals without a request", "Партнёр может назначать условия своим рефералам без заявки")}</small></span></label>
      {value.b2bEnabled ? <>
        <label className="field partner-b2b-limit"><span>{t("Maximum customer discount", "Максимальная скидка клиенту")}</span><div className="percent-input"><input id={`${props.idPrefix}-b2b-maximum`} name="b2bMaximumPercent" type="number" inputMode="decimal" autoComplete="off" min="0" max="95" step="0.01" value={value.b2bMaximum} onChange={(event) => onChange({ ...value, b2bMaximum: event.target.value })} disabled={props.disabled} /><i>%</i></div></label>
        <label className="admin-check"><input name={`${props.idPrefix}-b2bCanDelegate`} type="checkbox" checked={value.b2bCanDelegate} onChange={(event) => onChange({ ...value, b2bCanDelegate: event.target.checked })} disabled={props.disabled} /><span><b>{t("May delegate B2B", "Может делегировать B2B")}</b><small>{t("A Team member receives a smaller ceiling", "Участник Team получает меньший лимит")}</small></span></label>
      </> : null}
    </fieldset>
  </>;
}

export type PartnerOnboardingTarget = { id: string; email: string };

function PartnerOnboardingDialogForm(props: {
  target: PartnerOnboardingTarget;
  onClose: () => void;
  onCreated: () => void;
}) {
  const { t } = useI18n();
  const [terms, setTerms] = useState(DEFAULT_PARTNER_TERMS);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setError(null);
    const payload = partnerOnboardingPayload(terms);
    if (!payload) {
      setError(t("Check the percentages: commission ≤ 100%, Team share ≤ 20%, B2B discount ≤ 95%.", "Проверьте проценты: комиссия ≤ 100%, Team-доля ≤ 20%, B2B-скидка ≤ 95%."));
      window.requestAnimationFrame(() => document.getElementById("user-partner-commission")?.focus());
      return;
    }
    setBusy(true);
    try {
      await send(`/admin/users/${encodeURIComponent(props.target.id)}/referral-partner`, "POST", payload);
      toast(t("Partner access enabled", "Партнёрский доступ включён"));
      props.onCreated();
      props.onClose();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("Could not enable partner access.", "Не удалось включить партнёрский доступ."));
    } finally {
      setBusy(false);
    }
  }

  return <form className="partner-onboarding-form" onSubmit={submit} noValidate>
    <div aria-live="polite">{error ? <Banner kind="bad" title={t("Onboarding failed", "Подключение не выполнено")}>{error}</Banner> : null}</div>
    <div className="partner-identity-lock"><span>{t("Commerce account", "Commerce-аккаунт")}</span><b translate="no">{props.target.email}</b><small>{t("The dashboard login email becomes the only partner identity", "Email входа в Dashboard становится единственным идентификатором партнёра")}</small></div>
    <PartnerTermsFields idPrefix="user-partner" value={terms} onChange={setTerms} disabled={busy} />
    <div className="dlg-actions"><button type="button" className="btn ghost" disabled={busy} onClick={props.onClose}>{t("Cancel", "Отмена")}</button><button type="submit" className="btn" disabled={busy}>{busy ? t("Enabling…", "Подключаем…") : t("Enable Partner Access", "Сделать партнёром")}</button></div>
  </form>;
}

export function PartnerOnboardingDialog(props: {
  target: PartnerOnboardingTarget | null;
  onClose: () => void;
  onCreated: () => void;
}) {
  const { t } = useI18n();
  return <Modal open={props.target !== null} wide title={t("Enable Partner Access", "Сделать пользователя партнёром")} message={t("Set the platform commission and the maximum authority the partner can delegate.", "Задайте комиссию от платформы и максимальные полномочия, которые партнёр сможет делегировать.")} onClose={props.onClose}>
    {props.target ? <PartnerOnboardingDialogForm key={props.target.id} target={props.target} onClose={props.onClose} onCreated={props.onCreated} /> : null}
  </Modal>;
}
