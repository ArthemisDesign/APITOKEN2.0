"use client";

import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type ReactNode } from "react";
import { useI18n } from "@/components/i18n-provider";
import {
  api,
  ApiError,
  type ReferralActiveSnapshot,
  type ReferralAuthorityInput,
  type ReferralRequest,
  type ReferralSnapshot,
  type ReferralTeamMember,
} from "@/lib/api";
import { DASHBOARD_PROVIDERS, fallbackProvider } from "@/lib/providers";
import { formatNanoUsd, PageHeading } from "./shared";

const TABS = ["overview", "referrals", "team", "requests", "payouts"] as const;
type ReferralTab = typeof TABS[number];
type Language = "en" | "ru";

const copy = {
  en: {
    eyebrow: "Partner program", title: "Referrals", subtitle: "Your clients, Team, earnings and partner requests — inside the same account you use for the API.",
    loading: "Loading partner data…", loadError: "Partner data is temporarily unavailable.", retry: "Try again",
    ordinaryTitle: "Grow with apiToken.sale", ordinaryBody: "The partner program is enabled manually. Standard terms start at a 10% commission on paid usage from your referred clients. Your referrals and Team members are existing apiToken.sale accounts and are identified by their account email.",
    ordinaryPoint1: "Commission only on eligible paid usage", ordinaryPoint2: "Provider-level earnings and transparent payout periods", ordinaryPoint3: "A Team hierarchy with a retained share capped at 20%",
    requestAccess: "Request partner access", contactHint: "The button opens a conversation with @bozinodev in Telegram. Your Dashboard account remains the only program identity.",
    disabledTitle: "Partner access is paused", disabledBody: "Your account is still intact, but partner actions are disabled. Contact support to clarify or restore access.", contact: "Contact @bozinodev",
    overview: "Overview", referrals: "Referrals", team: "Team", requests: "Requests", payouts: "Payouts",
    available: "Available", earned30: "Net earned · 30 days", direct: "Direct earnings", teamIncome: "Retained Team share", payable: "Payable", fixedRate: "Your platform commission", fixedRateHint: "Set only by apiToken.sale",
    chartTitle: "Earnings by provider", chartWindow: "Last 30 days", noEarnings: "No partner earnings in this period yet.", providerSummary: "Provider breakdown", events: "events", earned: "Earned", spend: "Paid usage", adjustments: "Adjustments", net: "Net",
    programTerms: "How the Team share works", termsBody: "Your Team share is retained from a member’s fixed platform commission; it is not added on top. With $100 of eligible spend and a 10% commission, the pool is $10. A 20% retained share gives $8 to the member and $2 to the parent — the total remains $10.",
    referralList: "Referred accounts", referralListSub: "People are always shown by their current Commerce account email.", email: "Account email", type: "Type", discount: "Discount", attributed: "Attributed", noReferrals: "No referred accounts yet.", unknownEmail: "Email unavailable",
    teamTitle: "Your Team", teamSub: "Invite an existing apiToken.sale account by email and choose only what you retain from its commission.", invite: "Invite Team member", inviting: "Sending…", retainedShare: "Retained Team share", retainedHelp: "A percentage of the member’s commission that you retain (maximum {max}%).", memberRate: "Member platform commission", platformControlled: "10% by default · controlled by apiToken.sale", delegatedTeamLimit: "Member’s maximum retained share", allowInvites: "May invite their own Team", allowB2b: "May set B2B pricing", maxB2b: "Maximum B2B discount", allowB2bDelegate: "May delegate B2B permission", sendInvitation: "Send invitation", inviteSent: "Invitation sent.", existingOnly: "Only an active, existing apiToken.sale account can be invited.",
    activeMembers: "Active members", pendingInvites: "Pending invitations", referralsCount: "Referrals", memberNet: "Member net", myShare: "Your retained share", authority: "Permissions", edit: "Edit", save: "Save", saving: "Saving…", cancel: "Cancel", revoke: "Revoke", revokeInviteTitle: "Revoke invitation?", revokeInviteBody: "This permanently closes the pending invitation for {email}. The account can be invited again later.", confirmRevoke: "Revoke invitation", noTeam: "No Team members yet.", noInvites: "No pending invitations.", inviteExpires: "Expires", updateSaved: "Team settings saved.",
    requestsTitle: "Partner requests", requestsSub: "Ask apiToken.sale to change your commission or approve B2B conditions for one of your referrals.", commissionRequest: "Commission review", currentCommission: "Current commission", requestedCommission: "Requested commission", reason: "Business justification", reasonPlaceholder: "Describe the volume, pipeline or other reason for this request…", sendRequest: "Send request", requestSent: "Request sent for review.",
    b2bTitle: "B2B referral pricing", b2bBody: "Use direct pricing only when the platform granted that permission. Otherwise submit the same customer and terms for review.", customerEmail: "Referral account email", requestType: "Request type", conversion: "Convert to B2B", pricing: "Change B2B pricing", requestedDiscount: "Requested discount", requestReview: "Request review", applyDirectly: "Apply directly", pricingApplied: "B2B pricing applied.", ceiling: "Your ceiling", noDirectB2b: "Direct B2B pricing is not enabled for this account.",
    requestHistory: "Request history", requestHistorySub: "Decisions and execution status are shown without internal account identifiers.", request: "Request", customer: "Customer", status: "Status", created: "Created", noRequests: "No requests yet.",
    payoutTitle: "Payouts", payoutSub: "Wallet, current accrual, locked periods and payment history.", wallet: "BSC wallet (USDT)", walletHelp: "Payouts go only to the bound BSC address. Verify every character before saving.", saveWallet: "Save wallet", walletSaved: "Wallet saved.", currentPeriod: "Current period", nextPayout: "Next payout", lifetimeNet: "Lifetime net", lifetimePaid: "Paid", debt: "Debt after reversals", minimum: "Minimum payout", lockedPeriods: "Locked periods", unlocks: "Unlocks", periodHistory: "Period history", phase: "Phase", payoutDate: "Payout date", noPayouts: "No payout requests yet.", payoutRequests: "Payout requests", amount: "Amount", method: "Method", tx: "Transaction",
    invalidEmail: "Enter a valid account email.", invalidShare: "The retained share must be within your allowed maximum.", invalidReason: "Add a clear business justification.", invalidCommission: "Enter a commission from 0% to 100%.", invalidDiscount: "Enter a whole discount within your allowed ceiling.", invalidWallet: "Enter a valid 0x BSC wallet address.", mutationError: "The change could not be saved.",
  },
  ru: {
    eyebrow: "Партнёрская программа", title: "Рефералы", subtitle: "Ваши клиенты, команда, заработок и партнёрские заявки — внутри того же аккаунта, которым вы пользуетесь для API.",
    loading: "Загружаем партнёрские данные…", loadError: "Партнёрские данные временно недоступны.", retry: "Повторить",
    ordinaryTitle: "Развивайтесь вместе с apiToken.sale", ordinaryBody: "Доступ к партнёрской программе включается вручную. Стандартные условия начинаются с комиссии 10% от оплаченного использования привлечённых клиентов. Рефералы и участники команды — существующие аккаунты apiToken.sale, которые определяются по почте аккаунта.",
    ordinaryPoint1: "Комиссия только с оплаченного использования", ordinaryPoint2: "Разбивка заработка по провайдерам и прозрачные периоды выплат", ordinaryPoint3: "Командная иерархия с удерживаемой долей максимум 20%",
    requestAccess: "Запросить доступ партнёра", contactHint: "Кнопка откроет диалог с @bozinodev в Telegram. Единственной учётной записью программы остаётся ваш аккаунт Dashboard.",
    disabledTitle: "Партнёрский доступ приостановлен", disabledBody: "Ваш аккаунт и история сохранены, но партнёрские действия отключены. Напишите в поддержку, чтобы уточнить причину или восстановить доступ.", contact: "Написать @bozinodev",
    overview: "Обзор", referrals: "Рефералы", team: "Команда", requests: "Заявки", payouts: "Выплаты",
    available: "Доступно", earned30: "Чистый доход · 30 дней", direct: "Прямой доход", teamIncome: "Удержано с команды", payable: "К выплате", fixedRate: "Ваша комиссия от платформы", fixedRateHint: "Устанавливает только apiToken.sale",
    chartTitle: "Заработок по провайдерам", chartWindow: "Последние 30 дней", noEarnings: "В этом периоде партнёрского заработка пока нет.", providerSummary: "Разбивка по провайдерам", events: "событий", earned: "Заработано", spend: "Оплачено клиентами", adjustments: "Корректировки", net: "Чистыми",
    programTerms: "Как работает удержание с команды", termsBody: "Вы удерживаете Team-долю из фиксированной комиссии участника, а не получаете надбавку сверху. При $100 оплаченного расхода и комиссии 10% общий пул равен $10. Удержание 20% оставит участнику $8 и даст родителю $2 — общая выплата останется $10.",
    referralList: "Привлечённые аккаунты", referralListSub: "Люди везде отображаются по актуальной почте Commerce-аккаунта.", email: "Почта аккаунта", type: "Тип", discount: "Скидка", attributed: "Привлечён", noReferrals: "Привлечённых аккаунтов пока нет.", unknownEmail: "Почта недоступна",
    teamTitle: "Ваша команда", teamSub: "Пригласите существующий аккаунт apiToken.sale по почте и выберите только долю, которую будете удерживать из его комиссии.", invite: "Пригласить в команду", inviting: "Отправляем…", retainedShare: "Удерживаемая Team-доля", retainedHelp: "Процент от комиссии участника, который вы удерживаете (максимум {max}%).", memberRate: "Комиссия участника от платформы", platformControlled: "По умолчанию 10% · задаёт apiToken.sale", delegatedTeamLimit: "Максимальное удержание участника", allowInvites: "Может приглашать свою команду", allowB2b: "Может назначать B2B-условия", maxB2b: "Максимальная B2B-скидка", allowB2bDelegate: "Может делегировать право B2B", sendInvitation: "Отправить приглашение", inviteSent: "Приглашение отправлено.", existingOnly: "Пригласить можно только существующий активный аккаунт apiToken.sale.",
    activeMembers: "Участники", pendingInvites: "Ожидающие приглашения", referralsCount: "Рефералы", memberNet: "Доход участника", myShare: "Ваше удержание", authority: "Полномочия", edit: "Изменить", save: "Сохранить", saving: "Сохраняем…", cancel: "Отмена", revoke: "Отозвать", revokeInviteTitle: "Отозвать приглашение?", revokeInviteBody: "Ожидающее приглашение для {email} будет закрыто. Позже аккаунт можно будет пригласить снова.", confirmRevoke: "Отозвать приглашение", noTeam: "В команде пока никого нет.", noInvites: "Ожидающих приглашений нет.", inviteExpires: "Истекает", updateSaved: "Настройки участника сохранены.",
    requestsTitle: "Партнёрские заявки", requestsSub: "Запросите изменение своей комиссии или B2B-условия для привлечённого аккаунта.", commissionRequest: "Пересмотр комиссии", currentCommission: "Текущая комиссия", requestedCommission: "Желаемая комиссия", reason: "Обоснование", reasonPlaceholder: "Опишите объём, воронку или другую причину запроса…", sendRequest: "Отправить заявку", requestSent: "Заявка отправлена на рассмотрение.",
    b2bTitle: "B2B-условия реферала", b2bBody: "Назначайте условия напрямую только при выданном платформой разрешении. Иначе отправьте те же данные на рассмотрение.", customerEmail: "Почта аккаунта реферала", requestType: "Тип заявки", conversion: "Перевести в B2B", pricing: "Изменить B2B-условия", requestedDiscount: "Запрашиваемая скидка", requestReview: "Запросить согласование", applyDirectly: "Применить напрямую", pricingApplied: "B2B-условия применены.", ceiling: "Ваш максимум", noDirectB2b: "Самостоятельное назначение B2B-условий для этого аккаунта не включено.",
    requestHistory: "История заявок", requestHistorySub: "Решения и исполнение показаны без внутренних идентификаторов аккаунтов.", request: "Заявка", customer: "Клиент", status: "Статус", created: "Создана", noRequests: "Заявок пока нет.",
    payoutTitle: "Выплаты", payoutSub: "Кошелёк, текущие начисления, заблокированные периоды и история выплат.", wallet: "BSC-кошелёк (USDT)", walletHelp: "Выплаты отправляются только на привязанный BSC-адрес. Проверьте каждый символ перед сохранением.", saveWallet: "Сохранить кошелёк", walletSaved: "Кошелёк сохранён.", currentPeriod: "Текущий период", nextPayout: "Следующая выплата", lifetimeNet: "За всё время", lifetimePaid: "Выплачено", debt: "Долг после возвратов", minimum: "Минимальная выплата", lockedPeriods: "Заблокированные периоды", unlocks: "Разблокировка", periodHistory: "История периодов", phase: "Этап", payoutDate: "Дата выплаты", noPayouts: "Заявок на выплату пока нет.", payoutRequests: "Заявки на выплату", amount: "Сумма", method: "Метод", tx: "Транзакция",
    invalidEmail: "Введите корректную почту аккаунта.", invalidShare: "Удерживаемая доля должна быть в пределах доступного максимума.", invalidReason: "Добавьте понятное обоснование.", invalidCommission: "Введите комиссию от 0% до 100%.", invalidDiscount: "Введите целую скидку в пределах доступного максимума.", invalidWallet: "Введите корректный адрес BSC-кошелька, начинающийся с 0x.", mutationError: "Не удалось сохранить изменение.",
  },
} as const;

function tabFromUrl(): ReferralTab {
  if (typeof window === "undefined") return "overview";
  const value = new URLSearchParams(window.location.search).get("tab");
  return TABS.includes(value as ReferralTab) ? value as ReferralTab : "overview";
}

function pct(bps: number, locale: string): string {
  return `${(bps / 100).toLocaleString(locale, { maximumFractionDigits: 2 })}%`;
}

function date(value: string | null, locale: string): string {
  if (!value) return "—";
  const parsed = new Date(value);
  return Number.isNaN(parsed.valueOf()) ? "—" : new Intl.DateTimeFormat(locale, { dateStyle: "medium" }).format(parsed);
}

function validEmail(value: string): boolean {
  return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value.trim()) && value.length <= 320;
}

function mutationKey(): string {
  return typeof crypto.randomUUID === "function" ? crypto.randomUUID() : `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

function errorMessage(cause: unknown, fallback: string): string {
  if (cause instanceof ApiError && cause.status < 500 && cause.message) return cause.message;
  return fallback;
}

export function Referral() {
  const { language } = useI18n();
  const text = copy[language];
  const [snapshot, setSnapshot] = useState<ReferralSnapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState(false);
  const [tab, setTab] = useState<ReferralTab>(tabFromUrl);

  const load = useCallback(async (quiet = false) => {
    if (!quiet) setLoading(true);
    setLoadError(false);
    try { setSnapshot(await api.referral()); }
    catch { setLoadError(true); }
    finally { if (!quiet) setLoading(false); }
  }, []);

  useEffect(() => {
    let active = true;
    void api.referral()
      .then((next) => { if (active) setSnapshot(next); })
      .catch(() => { if (active) setLoadError(true); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, []);
  useEffect(() => {
    const sync = () => setTab(tabFromUrl());
    window.addEventListener("popstate", sync);
    return () => window.removeEventListener("popstate", sync);
  }, []);

  function selectTab(next: ReferralTab) {
    setTab(next);
    const url = new URL(window.location.href);
    url.searchParams.set("view", "referral");
    if (next === "overview") url.searchParams.delete("tab");
    else url.searchParams.set("tab", next);
    window.history.pushState(null, "", `${url.pathname}${url.search}${url.hash}`);
  }

  if (loading) return <section className="panel referral-panel"><PageHeading eyebrow={text.eyebrow} title={text.title} subtitle={text.subtitle} /><div className="referral-loading" role="status"><span className="spinner" />{text.loading}</div></section>;
  if (loadError || !snapshot) return <section className="panel referral-panel"><PageHeading eyebrow={text.eyebrow} title={text.title} subtitle={text.subtitle} /><div className="empty-box" role="alert"><p>{text.loadError}</p><button type="button" className="btn btn-ghost" onClick={() => void load()}>{text.retry}</button></div></section>;
  if (snapshot.state === "unavailable") return <OrdinaryState language={language} />;
  if (snapshot.state === "disabled") return <DisabledState language={language} />;

  return <section className="panel referral-panel">
    <PageHeading eyebrow={text.eyebrow} title={text.title} subtitle={text.subtitle} />
    <nav className="referral-tabs" aria-label={text.title}>
      {TABS.map((item) => <button key={item} type="button" className={item === tab ? "on" : ""} aria-current={item === tab ? "page" : undefined} onClick={() => selectTab(item)}>{text[item]}</button>)}
    </nav>
    {tab === "overview" && <PartnerOverview snapshot={snapshot} language={language} />}
    {tab === "referrals" && <ReferralAccounts snapshot={snapshot} language={language} />}
    {tab === "team" && <Team snapshot={snapshot} language={language} refresh={() => load(true)} />}
    {tab === "requests" && <Requests snapshot={snapshot} language={language} refresh={() => load(true)} />}
    {tab === "payouts" && <Payouts snapshot={snapshot} language={language} refresh={() => load(true)} />}
  </section>;
}

function OrdinaryState({ language }: { language: Language }) {
  const text = copy[language];
  return <section className="panel referral-panel"><PageHeading eyebrow={text.eyebrow} title={text.title} subtitle={text.subtitle} />
    <div className="referral-access-card card">
      <div><span className="overview-status setup"><i />{text.eyebrow}</span><h2>{text.ordinaryTitle}</h2><p>{text.ordinaryBody}</p></div>
      <ul><li>{text.ordinaryPoint1}</li><li>{text.ordinaryPoint2}</li><li>{text.ordinaryPoint3}</li></ul>
      <a className="btn btn-primary" href="https://t.me/bozinodev" target="_blank" rel="noreferrer">{text.requestAccess}</a>
      <small>{text.contactHint}</small>
    </div>
  </section>;
}

function DisabledState({ language }: { language: Language }) {
  const text = copy[language];
  return <section className="panel referral-panel"><PageHeading eyebrow={text.eyebrow} title={text.title} subtitle={text.subtitle} />
    <div className="referral-access-card card referral-disabled-card"><div><span className="overview-status warning"><i />{text.disabledTitle}</span><h2>{text.disabledTitle}</h2><p>{text.disabledBody}</p></div><a className="btn btn-ghost" href="https://t.me/bozinodev" target="_blank" rel="noreferrer">{text.contact}</a></div>
  </section>;
}

function PartnerOverview({ snapshot, language }: { snapshot: ReferralActiveSnapshot; language: Language }) {
  const text = copy[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  return <div className="referral-tab-panel">
    <div className="ov-stats bill4 referral-stats">
      <Metric label={text.available} value={formatNanoUsd(snapshot.totals.availableNano, locale)} detail={text.payable} accent />
      <Metric label={text.earned30} value={formatNanoUsd(snapshot.totals.last30dNetNano, locale)} detail={`${text.adjustments}: ${formatNanoUsd(snapshot.totals.last30dAdjustmentNano, locale)}`} />
      <Metric label={text.direct} value={formatNanoUsd(snapshot.totals.directNetNano, locale)} detail={text.earned} />
      <Metric label={text.teamIncome} value={formatNanoUsd(snapshot.totals.overrideNetNano, locale)} detail={text.retainedShare} />
    </div>
    <div className="referral-rate-strip">
      <div><span>{text.fixedRate}</span><strong>{pct(snapshot.membership.commissionBps, locale)}</strong><small>{text.fixedRateHint}</small></div>
      <div><span>{text.retainedShare}</span><strong>{pct(snapshot.membership.teamOverrideMaxBps, locale)}</strong><small>{text.ceiling}</small></div>
      <div><span>{text.referrals}</span><strong>{snapshot.referrals.length.toLocaleString(locale)}</strong><small>{text.email}</small></div>
      <div><span>{text.team}</span><strong>{snapshot.team.length.toLocaleString(locale)}</strong><small>{text.activeMembers}</small></div>
    </div>
    <EarningsChart snapshot={snapshot} language={language} />
    <div className="banner referral-terms"><b>{text.programTerms}</b><span> {text.termsBody}</span></div>
  </div>;
}

function Metric({ label, value, detail, accent = false }: { label: string; value: string; detail: string; accent?: boolean }) {
  return <div className="ovstat"><span className="dlabel">{label}</span><b className={`num${accent ? " accent" : ""}`}>{value}</b><span className="dtrend">{detail}</span></div>;
}

function EarningsChart({ snapshot, language }: { snapshot: ReferralActiveSnapshot; language: Language }) {
  const text = copy[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const registry = new Map(DASHBOARD_PROVIDERS.map((provider) => [provider.id, provider]));
  const metadata = (id: string) => registry.get(id) ?? fallbackProvider(id, id === "unattributed" ? (language === "ru" ? "Без провайдера" : "Unattributed") : id);
  const points = snapshot.earnings.providerDaily.map((point) => ({
    date: point.date,
    providers: point.providers.map((provider) => ({ id: provider.providerId ?? "unattributed", events: provider.events, spend: BigInt(provider.spendNano), earned: BigInt(provider.earnedNano) })),
  }));
  const ids = [...new Set(points.flatMap((point) => point.providers.filter((provider) => provider.earned > 0n).map((provider) => provider.id)))];
  const providerOrder = new Map(DASHBOARD_PROVIDERS.map((provider, index) => [provider.id, index]));
  ids.sort((left, right) => (providerOrder.get(left) ?? 999) - (providerOrder.get(right) ?? 999) || left.localeCompare(right));
  const providers = ids.map(metadata);
  const totals = points.map((point) => point.providers.reduce((sum, provider) => sum + provider.earned, 0n));
  const max = totals.reduce((value, item) => item > value ? item : value, 0n);
  const [hover, setHover] = useState<number | null>(null);
  const marks = points.length === 0 ? [] : [...new Set([0, Math.floor((points.length - 1) / 2), points.length - 1])];
  const providerTotals = snapshot.earnings.providers.map((provider) => ({ ...provider, id: provider.providerId ?? "unattributed", earned: BigInt(provider.earnedNano), spend: BigInt(provider.spendNano) })).sort((a, b) => a.earned > b.earned ? -1 : a.earned < b.earned ? 1 : 0);

  return <div className="usage-graph referral-earnings-graph">
    <div className="uchart">
      <div className="uchart-head"><b>{text.chartTitle}</b><div className="uchart-head-meta"><span className="uchart-window">{text.chartWindow}</span><div className="usage-chart-legend" aria-label={text.providerSummary}>{providers.map((provider) => <span key={provider.id}><i style={{ background: provider.color }} />{provider.name}</span>)}</div></div></div>
      {max === 0n ? <div className="uchart-empty">{text.noEarnings}</div> : <div className="uchart-grid">
        <div className="uchart-yaxis"><span>{formatNanoUsd(max, locale, 0, 2)}</span><span>{formatNanoUsd(max / 2n, locale, 0, 2)}</span><span>$0</span></div>
        <div className="uchart-plotwrap"><div className="uchart-lines"><i /><i /><i /></div>
          <div className="uchart-plot" onMouseLeave={(event) => { if (!event.currentTarget.contains(document.activeElement)) setHover(null); }}>
            {points.map((point, index) => <button type="button" key={`${point.date}-${index}`} className={`uchart-col${hover === index ? " is-hover" : ""}`} aria-label={[`${date(point.date, locale)}. ${text.earned}: ${formatNanoUsd(totals[index] ?? 0n, locale)}`, ...point.providers.filter((item) => item.earned > 0n).map((item) => `${metadata(item.id).name}: ${formatNanoUsd(item.earned, locale)}`)].join(". ")} onMouseEnter={() => setHover(index)} onFocus={() => setHover(index)} onBlur={() => setHover((current) => current === index ? null : current)} onClick={() => setHover((current) => current === index ? null : index)} onKeyDown={(event) => { if (event.key === "Escape") { setHover(null); event.currentTarget.blur(); } }}><div className="uchart-col-fill">{providers.map((provider) => { const item = point.providers.find((candidate) => candidate.id === provider.id); return item && item.earned > 0n ? <div className="uchart-seg" key={provider.id} style={{ height: `${Number(item.earned * 10_000n / max) / 100}%`, background: provider.color }} /> : null; })}</div></button>)}
            {hover !== null && points[hover] && (totals[hover] ?? 0n) > 0n && <div className="chart-tip" role="tooltip" style={{ left: `${Math.min(92, Math.max(8, (hover + .5) / points.length * 100))}%`, bottom: `${Number((totals[hover] ?? 0n) * 10_000n / max) / 100}%` }}><div className="chart-tip-h">{date(points[hover]!.date, locale)}</div>{providers.map((provider) => { const item = points[hover]!.providers.find((candidate) => candidate.id === provider.id); return item && item.earned > 0n ? <div className="chart-tip-row" key={provider.id}><span className="chart-tip-dot" style={{ background: provider.color }} /><span className="chart-tip-nm">{provider.name}</span><b>{formatNanoUsd(item.earned, locale)}</b></div> : null; })}<div className="chart-tip-total"><span>{text.earned}</span><b>{formatNanoUsd(totals[hover] ?? 0n, locale)}</b></div></div>}
          </div>
          <div className="uchart-axis">{marks.map((mark) => <span key={mark} style={{ left: `${(mark + .5) / points.length * 100}%` }}>{date(points[mark]!.date, locale)}</span>)}</div>
        </div>
      </div>}
    </div>
    <div className="usum"><span className="usum-t">{text.providerSummary}</span>{providerTotals.length === 0 ? <div className="referral-summary-empty">{text.noEarnings}</div> : providerTotals.map((provider) => { const item = metadata(provider.id); return <div className="usum-row referral-provider-row" key={provider.id}><span><i style={{ background: item.color }} />{item.name}<small>{provider.events.toLocaleString(locale)} {text.events} · {formatNanoUsd(provider.spend, locale)} {text.spend.toLocaleLowerCase()}</small></span><b>{formatNanoUsd(provider.earned, locale)}</b></div>; })}</div>
  </div>;
}

function ReferralAccounts({ snapshot, language }: { snapshot: ReferralActiveSnapshot; language: Language }) {
  const text = copy[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  return <div className="referral-tab-panel"><SectionHead title={text.referralList} sub={text.referralListSub} /><div className="table-scroll"><table className="mtable referral-table"><thead><tr><th>{text.email}</th><th>{text.type}</th><th className="tnum">{text.discount}</th><th className="tnum">{text.spend}</th><th className="tnum">{text.net}</th><th>{text.attributed}</th></tr></thead><tbody>{snapshot.referrals.length === 0 ? <tr><td colSpan={6} className="empty-cell">{text.noReferrals}</td></tr> : snapshot.referrals.map((item, index) => <tr key={`${item.email ?? "unknown"}-${index}`}><td><span className="referral-email" translate="no">{item.email ?? text.unknownEmail}</span></td><td><Status value={item.customerType?.toUpperCase() ?? "—"} /></td><td className="tnum">{item.discountBps === null ? "—" : pct(item.discountBps, locale)}</td><td className="tnum">{formatNanoUsd(item.spendNano, locale)}</td><td className="tnum referral-positive">{formatNanoUsd(item.netNano, locale)}</td><td>{date(item.attributedAt, locale)}</td></tr>)}</tbody></table></div></div>;
}

function Team({ snapshot, language, refresh }: { snapshot: ReferralActiveSnapshot; language: Language; refresh(): Promise<void> }) {
  const text = copy[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const maxShare = Math.min(2_000, snapshot.membership.teamOverrideMaxBps);
  const maxB2b = snapshot.membership.b2bMaxDiscountBps;
  const initialAuthority = useMemo<ReferralAuthorityInput>(() => ({ teamOverrideMaxBps: 0, teamInvitesEnabled: false, b2bEnabled: false, b2bMaxDiscountBps: 0, b2bCanDelegate: false }), []);
  const [email, setEmail] = useState("");
  const [share, setShare] = useState(0);
  const [authority, setAuthority] = useState(initialAuthority);
  const [editing, setEditing] = useState<ReferralTeamMember | null>(null);
  const [revoking, setRevoking] = useState<{ id: string; email: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<{ kind: "ok" | "bad"; message: string } | null>(null);

  async function invite(event: FormEvent) {
    event.preventDefault(); setNotice(null);
    if (!validEmail(email)) return setNotice({ kind: "bad", message: text.invalidEmail });
    if (share < 0 || share > maxShare || authority.teamOverrideMaxBps > maxShare) return setNotice({ kind: "bad", message: text.invalidShare });
    setBusy(true);
    try { await api.referralInviteTeam({ email: email.trim(), overrideBps: share, authority }); setEmail(""); setShare(0); setAuthority(initialAuthority); await refresh(); setNotice({ kind: "ok", message: text.inviteSent }); }
    catch (cause) { setNotice({ kind: "bad", message: errorMessage(cause, text.mutationError) }); }
    finally { setBusy(false); }
  }

  async function revoke(id: string): Promise<boolean> {
    setBusy(true); setNotice(null);
    try { await api.referralRevokeInvitation(id); await refresh(); return true; }
    catch (cause) { setNotice({ kind: "bad", message: errorMessage(cause, text.mutationError) }); return false; }
    finally { setBusy(false); }
  }

  return <div className="referral-tab-panel"><SectionHead title={text.teamTitle} sub={text.teamSub} />
    {snapshot.membership.teamInvitesEnabled ? <form className="referral-form-card card" onSubmit={invite}>
      <div className="referral-form-title"><div><h3>{text.invite}</h3><p>{text.existingOnly}</p></div><span className="overview-rate-chip">{text.retainedShare} ≤ {pct(maxShare, locale)}</span></div>
      <div className="referral-form-grid"><Field label={text.email}><input name="teamEmail" type="email" autoComplete="email" spellCheck={false} maxLength={320} value={email} onChange={(event) => setEmail(event.target.value)} placeholder="name@company.com" translate="no" /></Field><PercentField label={text.retainedShare} value={share} max={maxShare} onChange={setShare} help={text.retainedHelp.replace("{max}", String(maxShare / 100))} /><ReadOnly label={text.memberRate} value={text.platformControlled} /><PercentField label={text.delegatedTeamLimit} value={authority.teamOverrideMaxBps} max={maxShare} onChange={(teamOverrideMaxBps) => setAuthority({ ...authority, teamOverrideMaxBps })} /></div>
      <AuthorityFields value={authority} maxB2b={maxB2b} onChange={setAuthority} language={language} />
      <div className="referral-form-actions"><button className="btn btn-primary" disabled={busy}>{busy ? text.inviting : text.sendInvitation}</button></div>
    </form> : <div className="banner">{language === "ru" ? "Приглашения в команду отключены для вашего аккаунта администратором." : "Team invitations are disabled for your account by an administrator."}</div>}
    <LiveNotice notice={notice} />
    <SectionHead title={text.activeMembers} sub={`${snapshot.team.length.toLocaleString(locale)} · ${text.email}`} compact />
    <div className="table-scroll"><table className="mtable referral-table team-table"><thead><tr><th>{text.email}</th><th className="tnum">{text.retainedShare}</th><th className="tnum">{text.referralsCount}</th><th className="tnum">{text.memberNet}</th><th className="tnum">{text.myShare}</th><th>{text.authority}</th></tr></thead><tbody>{snapshot.team.length === 0 ? <tr><td colSpan={6} className="empty-cell">{text.noTeam}</td></tr> : snapshot.team.map((member, index) => <tr key={`${member.email ?? "unknown"}-${index}`}><td><span className="referral-email" translate="no">{member.email ?? text.unknownEmail}</span><small>{pct(member.commissionBps, locale)} {text.fixedRate.toLocaleLowerCase()}</small></td><td className="tnum">{pct(member.overrideBps, locale)}</td><td className="tnum">{member.referredUsers.toLocaleString(locale)}</td><td className="tnum">{formatNanoUsd(member.theirNetNano, locale)}</td><td className="tnum referral-positive">{formatNanoUsd(member.myOverrideNetNano, locale)}</td><td><button type="button" className="btn btn-ghost btn-sm" disabled={!member.email || busy} onClick={() => setEditing(member)}>{text.edit}</button></td></tr>)}</tbody></table></div>
    <SectionHead title={text.pendingInvites} sub={text.existingOnly} compact />
    <div className="referral-invites">{snapshot.invitations.filter((item) => !item.consumedAt && !item.revokedAt).length === 0 ? <div className="empty-box">{text.noInvites}</div> : snapshot.invitations.filter((item) => !item.consumedAt && !item.revokedAt).map((item) => <article className="referral-invite" key={item.id}><div><strong translate="no">{item.email ?? text.unknownEmail}</strong><span>{text.retainedShare}: {pct(item.overrideBps, locale)} · {text.inviteExpires}: {date(item.expiresAt, locale)}</span></div><button type="button" className="btn btn-ghost btn-sm" disabled={busy} onClick={() => setRevoking({ id: item.id, email: item.email ?? text.unknownEmail })}>{text.revoke}</button></article>)}</div>
    {editing && <TeamEditor member={editing} parent={snapshot} language={language} busy={busy} onClose={() => setEditing(null)} onSave={async (patch) => { if (!editing.email) return; setBusy(true); setNotice(null); try { await api.referralUpdateTeam({ email: editing.email, ...patch }); await refresh(); setEditing(null); setNotice({ kind: "ok", message: text.updateSaved }); } catch (cause) { setNotice({ kind: "bad", message: errorMessage(cause, text.mutationError) }); } finally { setBusy(false); } }} />}
    {revoking && <ConfirmDialog title={text.revokeInviteTitle} body={text.revokeInviteBody.replace("{email}", revoking.email)} confirm={text.confirmRevoke} cancel={text.cancel} busyLabel={text.saving} busy={busy} onClose={() => setRevoking(null)} onConfirm={async () => { if (await revoke(revoking.id)) setRevoking(null); }} />}
  </div>;
}

function AuthorityFields({ value, maxB2b, onChange, language }: { value: ReferralAuthorityInput; maxB2b: number; onChange(value: ReferralAuthorityInput): void; language: Language }) {
  const text = copy[language];
  return <div className="referral-authority-grid"><Toggle name="teamInvitesEnabled" label={text.allowInvites} checked={value.teamInvitesEnabled} onChange={(teamInvitesEnabled) => onChange({ ...value, teamInvitesEnabled })} /><Toggle name="b2bEnabled" label={text.allowB2b} checked={value.b2bEnabled} onChange={(b2bEnabled) => onChange({ ...value, b2bEnabled, b2bMaxDiscountBps: b2bEnabled ? value.b2bMaxDiscountBps : 0, b2bCanDelegate: b2bEnabled ? value.b2bCanDelegate : false })} />{value.b2bEnabled && <PercentField label={text.maxB2b} value={value.b2bMaxDiscountBps} max={maxB2b} onChange={(b2bMaxDiscountBps) => onChange({ ...value, b2bMaxDiscountBps })} />}{value.b2bEnabled && <Toggle name="b2bCanDelegate" label={text.allowB2bDelegate} checked={value.b2bCanDelegate} onChange={(b2bCanDelegate) => onChange({ ...value, b2bCanDelegate })} />}</div>;
}

function TeamEditor({ member, parent, language, busy, onClose, onSave }: { member: ReferralTeamMember; parent: ReferralActiveSnapshot; language: Language; busy: boolean; onClose(): void; onSave(patch: { overrideBps: number } & ReferralAuthorityInput): Promise<void> }) {
  const text = copy[language];
  const maxShare = Math.min(2_000, parent.membership.teamOverrideMaxBps);
  const [share, setShare] = useState(member.overrideBps);
  const [authority, setAuthority] = useState<ReferralAuthorityInput>({ teamOverrideMaxBps: member.teamOverrideMaxBps, teamInvitesEnabled: member.teamInvitesEnabled, b2bEnabled: member.b2bEnabled, b2bMaxDiscountBps: member.b2bMaxDiscountBps, b2bCanDelegate: member.b2bCanDelegate });
  const closeRef = useModalFocus(onClose);
  return <div className="modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}><section className="key-modal referral-modal" role="dialog" aria-modal="true" aria-labelledby="team-editor-title"><div className="key-modal-head"><div><span className="eyebrow">{text.team}</span><h2 id="team-editor-title" translate="no">{member.email}</h2></div><button ref={closeRef} type="button" className="key-modal-close" aria-label={text.cancel} onClick={onClose}>×</button></div><div className="referral-form-grid"><PercentField label={text.retainedShare} value={share} max={maxShare} onChange={setShare} /><ReadOnly label={text.memberRate} value={`${pct(member.commissionBps, language === "ru" ? "ru-RU" : "en-US")} · ${text.fixedRateHint}`} /><PercentField label={text.delegatedTeamLimit} value={authority.teamOverrideMaxBps} max={maxShare} onChange={(teamOverrideMaxBps) => setAuthority({ ...authority, teamOverrideMaxBps })} /></div><AuthorityFields value={authority} maxB2b={parent.membership.b2bMaxDiscountBps} onChange={setAuthority} language={language} /><div className="key-modal-actions"><button type="button" className="btn btn-ghost" onClick={onClose}>{text.cancel}</button><button type="button" className="btn btn-primary" disabled={busy || share > maxShare || authority.teamOverrideMaxBps > maxShare} onClick={() => void onSave({ overrideBps: share, ...authority })}>{busy ? text.saving : text.save}</button></div></section></div>;
}

function useModalFocus(onClose: () => void) {
  const focusRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    focusRef.current?.focus();
    const handleKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") { onClose(); return; }
      if (event.key !== "Tab") return;
      const modal = focusRef.current?.closest<HTMLElement>("[role='dialog'],[role='alertdialog']");
      const controls = modal ? [...modal.querySelectorAll<HTMLElement>("button:not(:disabled),a[href],input:not(:disabled),select:not(:disabled),textarea:not(:disabled),[tabindex]:not([tabindex='-1'])")] : [];
      if (controls.length === 0) return;
      const first = controls[0]!;
      const last = controls[controls.length - 1]!;
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
    };
    document.addEventListener("keydown", handleKey);
    return () => { document.removeEventListener("keydown", handleKey); previous?.focus(); };
  }, [onClose]);
  return focusRef;
}

function ConfirmDialog({ title, body, confirm, cancel, busyLabel, busy, onClose, onConfirm }: { title: string; body: string; confirm: string; cancel: string; busyLabel: string; busy: boolean; onClose(): void; onConfirm(): Promise<void> }) {
  const cancelRef = useModalFocus(() => { if (!busy) onClose(); });
  return <div className="modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget && !busy) onClose(); }}><section className="key-modal key-revoke-modal" role="alertdialog" aria-modal="true" aria-labelledby="referral-confirm-title" aria-describedby="referral-confirm-body"><div className="key-modal-head"><div><span className="eyebrow">{confirm}</span><h2 id="referral-confirm-title">{title}</h2></div><button ref={cancelRef} type="button" className="key-modal-close" aria-label={cancel} disabled={busy} onClick={onClose}>×</button></div><p id="referral-confirm-body">{body}</p><div className="key-modal-actions"><button type="button" className="btn btn-ghost" disabled={busy} onClick={onClose}>{cancel}</button><button type="button" className="btn btn-danger" disabled={busy} onClick={() => void onConfirm()}>{busy ? busyLabel : confirm}</button></div></section></div>;
}

function Requests({ snapshot, language, refresh }: { snapshot: ReferralActiveSnapshot; language: Language; refresh(): Promise<void> }) {
  const text = copy[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const [commission, setCommission] = useState(Math.min(100, snapshot.membership.commissionBps / 100));
  const [commissionReason, setCommissionReason] = useState("");
  const [customerEmail, setCustomerEmail] = useState("");
  const [requestType, setRequestType] = useState<"b2b_conversion" | "b2b_pricing">("b2b_conversion");
  const [discount, setDiscount] = useState(Math.min(20, snapshot.membership.b2bMaxDiscountBps / 100 || 20));
  const [b2bReason, setB2bReason] = useState("");
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<{ kind: "ok" | "bad"; message: string } | null>(null);

  async function requestCommission(event: FormEvent) {
    event.preventDefault(); setNotice(null);
    if (!Number.isInteger(commission * 100) || commission < 0 || commission > 100) return setNotice({ kind: "bad", message: text.invalidCommission });
    if (!commissionReason.trim()) return setNotice({ kind: "bad", message: text.invalidReason });
    setBusy(true); try { await api.referralRequestCommission({ requestedCommissionBps: Math.round(commission * 100), reason: commissionReason.trim() }, mutationKey()); setCommissionReason(""); await refresh(); setNotice({ kind: "ok", message: text.requestSent }); } catch (cause) { setNotice({ kind: "bad", message: errorMessage(cause, text.mutationError) }); } finally { setBusy(false); }
  }
  async function submitB2b(direct: boolean) {
    setNotice(null);
    if (!validEmail(customerEmail)) return setNotice({ kind: "bad", message: text.invalidEmail });
    const ceiling = direct ? snapshot.membership.b2bMaxDiscountBps / 100 : 95;
    if (!Number.isInteger(discount) || discount < 0 || discount > ceiling) return setNotice({ kind: "bad", message: text.invalidDiscount });
    if (!direct && !b2bReason.trim()) return setNotice({ kind: "bad", message: text.invalidReason });
    setBusy(true); try { if (direct) await api.referralSetBusinessPricing({ customerEmail: customerEmail.trim(), discountPercent: discount }, mutationKey()); else await api.referralRequestB2B({ customerEmail: customerEmail.trim(), requestType, requestedDiscountBps: discount * 100, providers: {}, reason: b2bReason.trim() }, mutationKey()); setCustomerEmail(""); setB2bReason(""); await refresh(); setNotice({ kind: "ok", message: direct ? text.pricingApplied : text.requestSent }); } catch (cause) { setNotice({ kind: "bad", message: errorMessage(cause, text.mutationError) }); } finally { setBusy(false); }
  }
  return <div className="referral-tab-panel"><SectionHead title={text.requestsTitle} sub={text.requestsSub} />
    <div className="referral-request-grid">
      <form className="referral-form-card card" onSubmit={requestCommission}><div className="referral-form-title"><div><h3>{text.commissionRequest}</h3><p>{text.currentCommission}: {pct(snapshot.membership.commissionBps, locale)}</p></div></div><PercentField label={text.requestedCommission} value={commission * 100} max={10_000} onChange={(value) => setCommission(value / 100)} /><Field label={text.reason}><textarea name="commissionReason" autoComplete="off" rows={5} maxLength={4_000} value={commissionReason} onChange={(event) => setCommissionReason(event.target.value)} placeholder={text.reasonPlaceholder} /></Field><div className="referral-form-actions"><button className="btn btn-primary" disabled={busy}>{busy ? text.saving : text.sendRequest}</button></div></form>
      <div className="referral-form-card card"><div className="referral-form-title"><div><h3>{text.b2bTitle}</h3><p>{text.b2bBody}</p></div>{snapshot.membership.b2bEnabled && <span className="overview-rate-chip">{text.ceiling}: {pct(snapshot.membership.b2bMaxDiscountBps, locale)}</span>}</div><div className="referral-form-grid"><Field label={text.customerEmail}><input name="b2bCustomerEmail" type="email" autoComplete="email" spellCheck={false} maxLength={320} value={customerEmail} onChange={(event) => setCustomerEmail(event.target.value)} placeholder="client@company.com" translate="no" /></Field><Field label={text.requestType}><select name="b2bRequestType" value={requestType} onChange={(event) => setRequestType(event.target.value as typeof requestType)}><option value="b2b_conversion">{text.conversion}</option><option value="b2b_pricing">{text.pricing}</option></select></Field><PercentField label={text.requestedDiscount} value={discount * 100} max={(snapshot.membership.b2bEnabled ? snapshot.membership.b2bMaxDiscountBps : 9_500)} onChange={(value) => setDiscount(value / 100)} /></div><Field label={text.reason}><textarea name="b2bReason" autoComplete="off" rows={4} maxLength={4_000} value={b2bReason} onChange={(event) => setB2bReason(event.target.value)} placeholder={text.reasonPlaceholder} /></Field><div className="referral-form-actions"><button type="button" className="btn btn-ghost" disabled={busy} onClick={() => void submitB2b(false)}>{text.requestReview}</button><button type="button" className="btn btn-primary" disabled={busy || !snapshot.membership.b2bEnabled} title={!snapshot.membership.b2bEnabled ? text.noDirectB2b : undefined} onClick={() => void submitB2b(true)}>{text.applyDirectly}</button></div>{!snapshot.membership.b2bEnabled && <small className="referral-help">{text.noDirectB2b}</small>}</div>
    </div><LiveNotice notice={notice} />
    <SectionHead title={text.requestHistory} sub={text.requestHistorySub} compact /><RequestTable requests={snapshot.requests} language={language} />
  </div>;
}

function RequestTable({ requests, language }: { requests: ReferralRequest[]; language: Language }) {
  const text = copy[language]; const locale = language === "ru" ? "ru-RU" : "en-US";
  return <div className="table-scroll"><table className="mtable referral-table"><thead><tr><th>{text.request}</th><th>{text.customer}</th><th>{text.status}</th><th>{text.created}</th></tr></thead><tbody>{requests.length === 0 ? <tr><td className="empty-cell" colSpan={4}>{text.noRequests}</td></tr> : requests.map((item) => <tr key={item.id}><td><strong>{requestLabel(item.requestType, language)}</strong><small>{item.reason}</small></td><td><span className="referral-email" translate="no">{item.customerEmail ?? "—"}</span></td><td><Status value={statusLabel(item.status, language)} kind={item.status === "rejected" || item.status === "apply_failed" ? "bad" : item.status === "applied" || item.status === "approved" ? "ok" : "warn"} /></td><td>{date(item.createdAt, locale)}</td></tr>)}</tbody></table></div>;
}

function Payouts({ snapshot, language, refresh }: { snapshot: ReferralActiveSnapshot; language: Language; refresh(): Promise<void> }) {
  const text = copy[language]; const locale = language === "ru" ? "ru-RU" : "en-US";
  const [wallet, setWallet] = useState(snapshot.membership.payoutDetails?.address ?? "");
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<{ kind: "ok" | "bad"; message: string } | null>(null);
  async function save(event: FormEvent) { event.preventDefault(); setNotice(null); if (!/^0x[a-fA-F0-9]{40}$/.test(wallet)) return setNotice({ kind: "bad", message: text.invalidWallet }); setBusy(true); try { await api.referralUpdateWallet(wallet); await refresh(); setNotice({ kind: "ok", message: text.walletSaved }); } catch (cause) { setNotice({ kind: "bad", message: errorMessage(cause, text.mutationError) }); } finally { setBusy(false); } }
  return <div className="referral-tab-panel"><SectionHead title={text.payoutTitle} sub={text.payoutSub} />
    <div className="referral-payout-grid"><form className="referral-form-card card" onSubmit={save}><div className="referral-form-title"><div><h3>{text.wallet}</h3><p>{text.walletHelp}</p></div></div><Field label={text.wallet}><input name="payoutWallet" type="text" autoComplete="off" spellCheck={false} inputMode="text" value={wallet} onChange={(event) => setWallet(event.target.value.trim())} placeholder="0x…" translate="no" /></Field><div className="referral-form-actions"><button className="btn btn-primary" disabled={busy}>{busy ? text.saving : text.saveWallet}</button></div></form>
      <div className="usum referral-period-summary"><span className="usum-t">{text.currentPeriod}</span><div className="usum-row"><span>{date(snapshot.period.current.start, locale)} — {date(snapshot.period.current.end, locale)}</span><b className="accent">{formatNanoUsd(snapshot.period.current.netNano, locale)}</b></div><div className="usum-row"><span>{text.nextPayout} · {date(snapshot.period.nextPayout.date, locale)}</span><b>{formatNanoUsd(snapshot.period.nextPayout.estimatedNano, locale)}</b></div><div className="usum-row"><span>{text.lifetimeNet}</span><b>{formatNanoUsd(snapshot.period.lifetimeNetNano, locale)}</b></div><div className="usum-row"><span>{text.lifetimePaid}</span><b>{formatNanoUsd(snapshot.period.lifetimePaidNano, locale)}</b></div><div className="usum-row"><span>{text.debt}</span><b>{formatNanoUsd(snapshot.period.debtNano, locale)}</b></div><div className="usum-row"><span>{text.minimum}</span><b>{formatNanoUsd(snapshot.payoutPolicy.minPayoutNano, locale)}</b></div></div></div><LiveNotice notice={notice} />
    {snapshot.period.locked.length > 0 && <><SectionHead title={text.lockedPeriods} sub={`${snapshot.payoutPolicy.lockDays} ${language === "ru" ? "дней блокировки" : "day lock"}`} compact /><div className="referral-invites">{snapshot.period.locked.map((period) => <article className="referral-invite" key={period.key}><div><strong>{period.key}</strong><span>{text.unlocks}: {date(period.unlocksAt, locale)} · {text.adjustments}: {formatNanoUsd(period.adjustmentNano, locale)}</span></div><b>{formatNanoUsd(period.netNano, locale)}</b></article>)}</div></>}
    <SectionHead title={text.periodHistory} sub={text.payoutSub} compact /><div className="table-scroll"><table className="mtable referral-table"><thead><tr><th>{text.currentPeriod}</th><th>{text.phase}</th><th>{text.payoutDate}</th><th className="tnum">{text.earned}</th><th className="tnum">{text.adjustments}</th><th className="tnum">{text.net}</th></tr></thead><tbody>{snapshot.periodHistory.map((period) => <tr key={`${period.key}-${period.index}`}><td>{period.key} · {period.index}/2</td><td><Status value={periodPhaseLabel(period.phase, language)} /></td><td>{date(period.payoutDate, locale)}</td><td className="tnum">{formatNanoUsd(period.earnedNano, locale)}</td><td className="tnum">{formatNanoUsd(period.adjustmentNano, locale)}</td><td className="tnum referral-positive">{formatNanoUsd(period.netNano, locale)}</td></tr>)}</tbody></table></div>
    <SectionHead title={text.payoutRequests} sub={text.payoutSub} compact /><div className="table-scroll"><table className="mtable referral-table"><thead><tr><th>{text.created}</th><th>{text.method}</th><th>{text.status}</th><th className="tnum">{text.amount}</th><th>{text.tx}</th></tr></thead><tbody>{snapshot.payouts.length === 0 ? <tr><td className="empty-cell" colSpan={5}>{text.noPayouts}</td></tr> : snapshot.payouts.map((payout) => <tr key={payout.id}><td>{date(payout.requestedAt, locale)}</td><td>{payout.method === "bsc_usdt" ? "BSC · USDT" : payout.method}</td><td><Status value={payoutStatusLabel(payout.status, language)} /></td><td className="tnum">{formatNanoUsd(payout.amountNano, locale)}</td><td><span className="referral-email" translate="no">{payout.txHash ?? "—"}</span></td></tr>)}</tbody></table></div>
  </div>;
}

function SectionHead({ title, sub, compact = false }: { title: string; sub: string; compact?: boolean }) { return <div className={`dsec-head analytics-heading referral-section-head${compact ? " compact" : ""}`}><div><h2>{title}</h2><p>{sub}</p></div></div>; }
function Field({ label, children }: { label: string; children: ReactNode }) { return <label className="referral-field"><span>{label}</span>{children}</label>; }
function ReadOnly({ label, value }: { label: string; value: string }) { return <div className="referral-field referral-readonly"><span>{label}</span><strong>{value}</strong></div>; }
function PercentField({ label, value, max, help, onChange }: { label: string; value: number; max: number; help?: string; onChange(value: number): void }) { return <label className="referral-field"><span>{label}</span><div className="referral-percent-input"><input name={label.replaceAll(" ", "-")} type="number" min={0} max={max / 100} step={0.01} inputMode="decimal" autoComplete="off" value={value / 100} onChange={(event) => onChange(Math.round(Number(event.target.value || 0) * 100))} /><i>%</i></div>{help && <small>{help}</small>}</label>; }
function Toggle({ name, label, checked, onChange }: { name: string; label: string; checked: boolean; onChange(value: boolean): void }) { return <label className="referral-toggle"><input name={name} type="checkbox" checked={checked} onChange={(event) => onChange(event.target.checked)} /><span className={`tgl${checked ? " on" : ""}`} aria-hidden="true" /><b>{label}</b></label>; }
function Status({ value, kind }: { value: string; kind?: "ok" | "bad" | "warn" }) { return <span className={`referral-status${kind ? ` ${kind}` : ""}`}>{value}</span>; }
function LiveNotice({ notice }: { notice: { kind: "ok" | "bad"; message: string } | null }) { return <div className={`referral-live${notice ? ` ${notice.kind}` : ""}`} aria-live="polite">{notice?.message ?? ""}</div>; }
function requestLabel(value: ReferralRequest["requestType"], language: Language): string { const labels = language === "ru" ? { commission_change: "Изменение комиссии", b2b_conversion: "Перевод в B2B", b2b_pricing: "B2B-условия" } : { commission_change: "Commission change", b2b_conversion: "B2B conversion", b2b_pricing: "B2B pricing" }; return labels[value]; }
function statusLabel(value: ReferralRequest["status"], language: Language): string { const labels = language === "ru" ? { pending: "На рассмотрении", approved: "Одобрено", rejected: "Отклонено", applied: "Применено", apply_failed: "Ошибка применения" } : { pending: "Pending", approved: "Approved", rejected: "Rejected", applied: "Applied", apply_failed: "Apply failed" }; return labels[value]; }
function periodPhaseLabel(value: ReferralActiveSnapshot["periodHistory"][number]["phase"], language: Language): string { const labels = language === "ru" ? { accruing: "Начисляется", locked: "Заблокирован", payable: "К выплате", closed: "Закрыт" } : { accruing: "Accruing", locked: "Locked", payable: "Payable", closed: "Closed" }; return labels[value]; }
function payoutStatusLabel(value: ReferralActiveSnapshot["payouts"][number]["status"], language: Language): string { const labels = language === "ru" ? { requested: "Запрошена", approved: "Одобрена", paid: "Выплачена", rejected: "Отклонена" } : { requested: "Requested", approved: "Approved", paid: "Paid", rejected: "Rejected" }; return labels[value]; }
