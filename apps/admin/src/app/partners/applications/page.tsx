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
import type { AdminPartnerApplication, PartnerApplicationStatus, PartnerApplicationsPage } from "../types";

type Draft = { action: "approve" | "reject"; note: string; commission: string; teamShare: string; b2bMax: string };

const DEFAULT_DRAFT: Omit<Draft, "action"> = { note: "", commission: "10", teamShare: "20", b2bMax: "50" };

function statusKind(status: PartnerApplicationStatus): "ok" | "warn" | "bad" {
  if (status === "approved") return "ok";
  return status === "pending" ? "warn" : "bad";
}

function statusLabel(status: PartnerApplicationStatus, t: (en: string, ru: string) => string): string {
  if (status === "pending") return t("Pending", "Ожидает");
  return status === "approved" ? t("Approved", "Одобрена") : t("Rejected", "Отклонена");
}

export default function PartnerApplicationsPage() {
  const { lang, t } = useI18n();
  const locale = localeFor(lang);
  const router = useRouter();
  const search = useSearchParams();
  const statusParam = search.get("status");
  const status: "" | PartnerApplicationStatus = statusParam === "all"
    ? ""
    : statusParam === "approved" || statusParam === "rejected" ? statusParam : "pending";
  const [selected, setSelected] = useState<AdminPartnerApplication | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const params = new URLSearchParams({ limit: "100" });
  if (status) params.set("status", status);
  const path = `/admin/referral/applications?${params}`;
  const { data, isLoading, refresh } = useResource<PartnerApplicationsPage>(path);

  function open(application: AdminPartnerApplication, action: "approve" | "reject") {
    setSelected(application);
    setDraft({ action, ...DEFAULT_DRAFT });
    setError(null);
  }

  function fail(message: string, fieldId?: string) {
    setError(message);
    if (fieldId) window.requestAnimationFrame(() => document.getElementById(fieldId)?.focus());
  }

  async function decide(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!selected || !draft) return;
    setError(null);
    // The note is optional: a decision is attributable by the actor header on its own.
    const body: Record<string, unknown> = { action: draft.action, note: draft.note.trim() };
    if (draft.action === "approve") {
      const commissionBps = parsePercentBps(draft.commission, 10_000);
      const teamOverrideMaxBps = parsePercentBps(draft.teamShare, 2_000);
      const b2bMaxDiscountBps = parsePercentBps(draft.b2bMax, 9_500);
      if (commissionBps === null) return fail(t("Enter a commission from 0% to 100%.", "Введите комиссию от 0% до 100%."), "application-commission");
      if (teamOverrideMaxBps === null) return fail(t("The Team ceiling cannot exceed 20%.", "Потолок команды не может превышать 20%."), "application-team-share");
      if (b2bMaxDiscountBps === null || b2bMaxDiscountBps % 100 !== 0) return fail(t("B2B discounts are whole percents up to 95%.", "B2B-скидки задаются целыми процентами до 95%."), "application-b2b-max");
      body.commissionBps = commissionBps;
      body.authority = {
        teamOverrideMaxBps,
        teamInvitesEnabled: true,
        b2bEnabled: b2bMaxDiscountBps > 0,
        b2bMaxDiscountBps,
        b2bCanDelegate: b2bMaxDiscountBps > 0,
      };
    }
    setBusy(true);
    try {
      await send(`/admin/referral/applications/${selected.id}/decision`, "POST", body);
      toast(draft.action === "approve" ? t("Partner access enabled", "Партнёрский доступ включён") : t("Application rejected", "Заявка отклонена"));
      setSelected(null); setDraft(null); refresh();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("Decision failed.", "Не удалось сохранить решение."));
    } finally { setBusy(false); }
  }

  const items = data?.items ?? [];
  const pending = items.filter((item) => item.status === "pending").length;
  if (isLoading && !data) return <><PageHead title={t("Access applications", "Заявки на доступ")} sub={t("Loading review queue", "Загружаем очередь")} /><LoadingGrid label={t("Loading access applications", "Загрузка заявок на доступ")} /></>;

  return <>
    <PageHead
      title={t("Access applications", "Заявки на доступ")}
      sub={t("Accounts asking to join the partner program", "Аккаунты, которые просят доступ в партнёрскую программу")}
      badge={<Pill kind={pending ? "warn" : "ok"}>{pending} {t("pending", "ожидают")}</Pill>}
    />
    {!data ? <Banner kind="bad" title={t("Queue unavailable", "Очередь недоступна")}>{path}</Banner> : null}
    <div className="partner-request-toolbar">
      <label className="field"><span>{t("Status", "Статус")}</span>
        <select name="applicationStatus" value={status} onChange={(event) => router.replace(`/partners/applications?status=${event.target.value || "all"}`, { scroll: false })}>
          <option value="pending">{t("Pending", "Ожидают")}</option>
          <option value="approved">{t("Approved", "Одобрены")}</option>
          <option value="rejected">{t("Rejected", "Отклонены")}</option>
          <option value="">{t("All", "Все")}</option>
        </select>
      </label>
      <button type="button" className="btn ghost" onClick={refresh}>{t("Refresh", "Обновить")}</button>
    </div>
    <TableCard><table className="partner-requests-table"><thead><tr>
      <th className="left">{t("Account", "Аккаунт")}</th>
      <th className="left">{t("What they wrote", "Что написали")}</th>
      <th>{t("Status", "Статус")}</th>
      <th className="left">{t("Decision", "Решение")}</th>
      <th>{t("Submitted", "Отправлена")}</th>
      <th><span className="sr-only">{t("Actions", "Действия")}</span></th>
    </tr></thead><tbody>
      {items.length ? items.map((application) => <tr key={application.id}>
        <td className="left"><b translate="no">{application.email}</b></td>
        <td className="left"><div className="partner-request-reason" title={application.message}>{application.message || "—"}</div></td>
        <td><Pill kind={statusKind(application.status)}>{statusLabel(application.status, t)}</Pill></td>
        <td className="left">{application.reviewerNote ? <div className="sub partner-request-note">{application.reviewerActor}: {application.reviewerNote}</div> : "—"}</td>
        <td>{formatDate(application.createdAt, true, locale)}</td>
        <td>{application.status === "pending"
          ? <div className="actions"><button type="button" className="btn" onClick={() => open(application, "approve")}>{t("Review", "Рассмотреть")}</button><button type="button" className="btn bad" onClick={() => open(application, "reject")}>{t("Reject", "Отклонить")}</button></div>
          : "—"}</td>
      </tr>) : <EmptyRow columns={6} text={t("No applications in this view", "В этом представлении заявок нет")} />}
    </tbody></table></TableCard>

    <Modal open={Boolean(selected && draft)} wide title={t("Partner access application", "Заявка на партнёрский доступ")} message={selected?.email} onClose={() => { if (!busy) { setSelected(null); setDraft(null); } }}>
      {selected && draft ? <form className="partner-decision-form" onSubmit={decide} noValidate>
        <div aria-live="polite">{error ? <div className="partner-form-error" role="alert">{error}</div> : null}</div>
        <div className="partner-request-full-reason"><b>{t("What the account wrote", "Что написал аккаунт")}</b><p>{selected.message || t("No message", "Без сообщения")}</p></div>
        <div className="partner-decision-choice" role="group" aria-label={t("Decision", "Решение")}>
          <button type="button" className={`btn${draft.action === "approve" ? "" : " ghost"}`} aria-pressed={draft.action === "approve"} onClick={() => setDraft({ ...draft, action: "approve" })}>{t("Approve", "Одобрить")}</button>
          <button type="button" className={`btn${draft.action === "reject" ? " bad" : " ghost"}`} aria-pressed={draft.action === "reject"} onClick={() => setDraft({ ...draft, action: "reject" })}>{t("Reject", "Отклонить")}</button>
        </div>
        {draft.action === "approve" ? <div className="partner-approved-terms">
          <label className="field"><span>{t("Commission", "Комиссия")}</span><div className="percent-input"><input id="application-commission" name="applicationCommission" type="text" inputMode="decimal" autoComplete="off" value={draft.commission} onChange={(event) => setDraft({ ...draft, commission: event.target.value })} disabled={busy} /><i>%</i></div><small>{t("Standard terms start at 10%", "Стандартные условия начинаются с 10%")}</small></label>
          <label className="field"><span>{t("Team ceiling", "Потолок команды")}</span><div className="percent-input"><input id="application-team-share" name="applicationTeamShare" type="text" inputMode="decimal" autoComplete="off" value={draft.teamShare} onChange={(event) => setDraft({ ...draft, teamShare: event.target.value })} disabled={busy} /><i>%</i></div><small>{t("Platform hard maximum 20%", "Жёсткий максимум платформы 20%")}</small></label>
          <label className="field"><span>{t("B2B ceiling", "B2B-потолок")}</span><div className="percent-input"><input id="application-b2b-max" name="applicationB2bMax" type="text" inputMode="numeric" autoComplete="off" value={draft.b2bMax} onChange={(event) => setDraft({ ...draft, b2bMax: event.target.value.replace(/\D/g, "") })} disabled={busy} /><i>%</i></div><small>{t("The ceiling for this partner's own B2B terms; 0% switches B2B off", "Потолок собственных B2B-условий партнёра; 0% выключает B2B")}</small></label>
        </div> : null}
        <label className="field"><span>{t("Reviewer note — optional", "Комментарий проверяющего — необязательно")}</span><textarea id="application-note" name="applicationNote" autoComplete="off" rows={4} maxLength={2000} value={draft.note} onChange={(event) => setDraft({ ...draft, note: event.target.value })} disabled={busy} placeholder={t("Why this account is a good partner…", "Почему этот аккаунт подходит…")} /></label>
        <div className="dlg-actions">
          <button type="button" className="btn ghost" onClick={() => { setSelected(null); setDraft(null); }} disabled={busy}>{t("Cancel", "Отмена")}</button>
          <button type="submit" className={`btn${draft.action === "reject" ? " bad" : ""}`} disabled={busy}>{busy ? t("Saving…", "Сохраняем…") : draft.action === "approve" ? t("Approve and enable", "Одобрить и включить") : t("Reject application", "Отклонить заявку")}</button>
        </div>
      </form> : null}
    </Modal>
  </>;
}
