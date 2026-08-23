"use client";

import { useState, type FormEvent } from "react";
import { Banner, EmptyRow, PageHead, Pill, SectionHeader, TableCard } from "@/components/ui";
import { send } from "@/lib/api";
import { formatDate } from "@/lib/format";
import { localeFor, useI18n } from "@/lib/i18n";
import { useResource } from "@/lib/resources";
import { toast } from "@/lib/toast";
import type { AdminUsersPage } from "../../users/users-lib";
import {
  DEFAULT_PARTNER_TERMS,
  PartnerOnboardingDialog,
  PartnerTermsFields,
  partnerOnboardingPayload,
  type PartnerOnboardingTarget,
  type PartnerTermsDraft,
} from "../partner-onboarding-form";

const ACCOUNT_EMAIL = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

export default function PartnerOnboardingPage() {
  const { lang, t } = useI18n();
  const locale = localeFor(lang);
  const [email, setEmail] = useState("");
  const [terms, setTerms] = useState<PartnerTermsDraft>(DEFAULT_PARTNER_TERMS);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Finding the account is part of the job: the same server-side user search the Users page uses,
  // so an operator never has to copy an email between two tabs.
  const [search, setSearch] = useState("");
  const [query, setQuery] = useState("");
  const [target, setTarget] = useState<PartnerOnboardingTarget | null>(null);
  const { data: found, isLoading: searching, refresh } = useResource<AdminUsersPage>(
    query ? `/admin/users?limit=20&offset=0&sort=created_at&dir=desc&q=${encodeURIComponent(query)}` : "",
  );

  function fail(message: string, fieldId: string) {
    setError(message);
    window.requestAnimationFrame(() => document.getElementById(fieldId)?.focus());
  }

  async function onboard(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setError(null);
    const accountEmail = email.trim().toLowerCase();
    if (!ACCOUNT_EMAIL.test(accountEmail) || accountEmail.length > 320) {
      fail(t("Enter the email used to sign in to the customer Dashboard.", "Введите email, который используется для входа в клиентский Dashboard."), "partner-account-email");
      return;
    }
    const payload = partnerOnboardingPayload(terms);
    if (!payload) {
      fail(t("Check the percentages: commission ≤ 100%, Team share ≤ 20%, B2B discount ≤ 95%.", "Проверьте проценты: комиссия ≤ 100%, Team-доля ≤ 20%, B2B-скидка ≤ 95%."), "partner-onboard-commission");
      return;
    }
    setBusy(true);
    try {
      await send("/admin/referral/partners", "POST", { email: accountEmail, ...payload });
      toast(t("Partner access enabled", "Партнёрский доступ включён"));
      setEmail("");
      setTerms(DEFAULT_PARTNER_TERMS);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("Could not enable partner access.", "Не удалось включить партнёрский доступ."));
    } finally {
      setBusy(false);
    }
  }

  return <>
    <PageHead title={t("Partner Onboarding", "Подключение партнёра")} sub={t("Enable an existing Commerce account and set its authority boundaries", "Подключите существующий Commerce-аккаунт и задайте границы его полномочий")} badge={<Pill kind="info">{t("Email only", "Только email")}</Pill>} />
    <div className="partner-identity-notice">
      <span aria-hidden="true">@</span>
      <div><b>{t("One account, one identity", "Один аккаунт — один идентификатор")}</b><p>{t("Use the same email the client uses at apitoken.sale. Access starts immediately after these terms are saved.", "Используйте тот же email, с которым клиент входит на apitoken.sale. Доступ включится сразу после сохранения условий.")}</p></div>
    </div>
    <div aria-live="polite">{error ? <Banner kind="bad" title={t("Onboarding Failed", "Подключение не выполнено")}>{error}</Banner> : null}</div>
    <SectionHeader title={t("Account & Terms", "Аккаунт и условия")} sub={t("The direct commission is platform-funded. A Team share is retained from a member’s commission, not added on top.", "Прямую комиссию платит платформа. Team-доля удерживается из комиссии участника, а не начисляется сверху.")} />
    <form className="partner-onboarding-form form-card" onSubmit={onboard} noValidate>
      <label className="field partner-onboarding-email"><span>{t("Dashboard Login Email", "Email входа в Dashboard")}</span><input id="partner-account-email" name="email" type="email" autoComplete="off" spellCheck={false} value={email} onChange={(event) => setEmail(event.target.value)} disabled={busy} placeholder="partner@example.com…" translate="no" /><small>{t("The account must already exist and be active in Commerce", "Аккаунт уже должен существовать и быть активным в Commerce")}</small></label>
      <PartnerTermsFields idPrefix="partner-onboard" value={terms} onChange={setTerms} disabled={busy} />
      <div className="partner-authority-actions"><span className="partner-terms-proof">$100 × 10% = $10 · Team 20% → $8 {t("member", "участнику")} + $2 {t("parent", "родителю")}</span><button className="btn" type="submit" disabled={busy}>{busy ? t("Enabling…", "Подключаем…") : t("Enable Partner Access", "Сделать партнёром")}</button></div>
    </form>

    <SectionHeader
      title={t("Find the account", "Найти аккаунт")}
      sub={t("Search the site's users by email or name and enable partner access from the row.", "Ищите пользователей сайта по email или имени и включайте партнёрский доступ прямо из строки.")}
    />
    <form
      className="partner-request-toolbar"
      onSubmit={(event) => { event.preventDefault(); setQuery(search.trim()); }}
    >
      <label className="field partner-user-search">
        <span>{t("Email or name", "Email или имя")}</span>
        <input
          id="partner-user-search"
          name="partnerUserSearch"
          type="search"
          autoComplete="off"
          spellCheck={false}
          value={search}
          onChange={(event) => setSearch(event.target.value)}
          placeholder={t("client@example.com", "client@example.com")}
          translate="no"
        />
      </label>
      <button type="submit" className="btn">{t("Search", "Найти")}</button>
      {query ? <button type="button" className="btn ghost" onClick={() => { setSearch(""); setQuery(""); }}>{t("Clear", "Сбросить")}</button> : null}
    </form>
    {query ? <TableCard>
      <table className="partner-requests-table">
        <thead><tr>
          <th className="left">{t("Account", "Аккаунт")}</th>
          <th>{t("Status", "Статус")}</th>
          <th>{t("Type", "Тип")}</th>
          <th>{t("Registered", "Регистрация")}</th>
          <th><span className="sr-only">{t("Actions", "Действия")}</span></th>
        </tr></thead>
        <tbody>
          {(found?.users ?? []).length ? (found?.users ?? []).map((user) => <tr key={user.id ?? user.email}>
            <td className="left"><b translate="no">{user.email ?? "—"}</b>{user.display_name ? <div className="sub" translate="no">{user.display_name}</div> : null}</td>
            <td><Pill kind={user.status === "active" ? "ok" : "bad"}>{user.status ?? "—"}</Pill></td>
            <td>{user.customer_type === "b2b" ? "B2B" : "B2C"}</td>
            <td>{user.created_at ? formatDate(user.created_at, false, locale) : "—"}</td>
            <td>{user.id && user.email && user.status === "active"
              ? <button type="button" className="btn" onClick={() => setTarget({ id: user.id!, email: user.email! })}>{t("Make partner", "Сделать партнёром")}</button>
              : <span className="sub">{t("Account is not active", "Аккаунт неактивен")}</span>}</td>
          </tr>) : <EmptyRow columns={5} text={searching ? t("Searching…", "Ищем…") : t("No accounts match this search", "По этому запросу аккаунтов нет")} />}
        </tbody>
      </table>
    </TableCard> : null}

    <PartnerOnboardingDialog target={target} onClose={() => setTarget(null)} onCreated={refresh} />
  </>;
}
