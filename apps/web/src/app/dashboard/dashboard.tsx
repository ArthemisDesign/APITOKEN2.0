"use client";

import Image from "next/image";
import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { useCallback, useEffect, useRef, useState, type CSSProperties, type FormEvent } from "react";
import {
  api, ApiError, type AccountView, type ApiKeyView, type AuthUser, type CheckoutView, type LedgerEntry, type TotpSetup, type UsageView,
} from "@/lib/api";
import { normalizeUsd } from "@/lib/money";
import { B2C_PRICING_MILESTONES, formatWholeUsd, pricingMilestoneProgress } from "@/lib/pricing-tiers";
import { useI18n } from "@/components/i18n-provider";
import { ThemeToggle } from "@/components/site-chrome";
import { SupportContent } from "@/components/compliance-pages";
import { dashboardCopy, type DashboardCopy } from "@/lib/dashboard-copy";
import { DOCS_URL } from "@/lib/site-links";
import { buildClaudeAgentHandoff, buildClaudeCodeCommands } from "@/lib/claude-connection";
import { checkoutAmountBucket, trackFirstProductEvent, trackProductEvent } from "@/lib/product-analytics";
import { dashboardHref, parseDashboardSection, type DashboardSection } from "./dashboard-route";

type Section = DashboardSection;
type KeyStatusFilter = "current" | "working" | "attention" | "disabled" | "all";
type OptionalDataSource = "keys" | "ledger" | "usage";

const NANO_PER_USD = 1_000_000_000n;
const BASIS_POINTS = 10_000n;
const CHECKOUT_ORIGINS: Record<CheckoutView["provider"], ReadonlySet<string>> = {
  cryptomus: new Set(["https://pay.cryptomus.com"]),
  platega: new Set(["https://pay.platega.io", "https://app.platega.io"]),
};
// Payment methods actually enabled on our Platega merchant (SBP + crypto). Each has an icon and a
// one-line description so it is obvious what it is; other Platega method ids are not available to us.
const PLATEGA_METHODS = [
  {
    id: 2, en: "SBP", ru: "СБП",
    enDesc: "Russian bank transfer (SBP)", ruDesc: "Банки России · перевод по СБП",
    logo: true,
    // Официальный знак СБП (Система быстрых платежей) как значок способа оплаты.
    icon: <svg viewBox="0 0 97.3 120" fill="none"><path d="M0 26.12l14.532 25.975v15.844L.017 93.863z" fill="#5b57a2" /><path d="M55.797 42.643l13.617-8.346 27.868-.026-41.485 25.414z" fill="#d90751" /><path d="M55.72 25.967l.077 34.39-14.566-8.95V0l14.49 25.967z" fill="#fab718" /><path d="M97.282 34.271l-27.869.026-13.693-8.33L41.231 0l56.05 34.271z" fill="#ed6f26" /><path d="M55.797 94.007V77.322l-14.566-8.78.008 51.458z" fill="#63b22f" /><path d="M69.38 85.737L14.531 52.095 0 26.12l97.223 59.583-27.844.034z" fill="#1487c9" /><path d="M41.24 120l14.556-25.993 13.583-8.27 27.843-.034z" fill="#017f36" /><path d="M.017 93.863l41.333-25.32-13.896-8.526-12.922 7.922z" fill="#984995" /></svg>,
  },
  {
    id: 11, en: "Card", ru: "Карта",
    enDesc: "Russian bank card (Mir)", ruDesc: "Карта РФ · Мир, эквайринг",
    icon: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round"><rect width="20" height="14" x="2" y="5" rx="2" /><path d="M2 10h20" /><path d="M6 15h4" /></svg>,
  },
  {
    id: 13, en: "Crypto", ru: "Криптовалюта",
    enDesc: "USDT and other coins", ruDesc: "USDT и другие монеты",
    icon: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round"><circle cx="8" cy="8" r="6" /><path d="M18.09 10.37A6 6 0 1 1 10.34 18" /><path d="M7 6h1v4" /><path d="m16.71 13.88.7.71-2.82 2.82" /></svg>,
  },
] as const;

const localDashboardCopy = {
  en: {
    logoutError: "Logout failed. Your server session is still active; please try again.", loggingOut: "Logging out…",
    invalidCheckoutUrl: "The payment provider returned an unsafe checkout address. Payment was not opened.",
    invalidWholeUsd: "Enter a positive whole USD amount using digits only, without decimals, signs, separators, or leading zeros.",
    editKey: "Edit", editKeyTitle: "Edit API key", editKeyHelp: "Update the name, spending limit, or expiration in one place. Limit changes apply to new requests immediately.", labelRequired: "Enter a label before saving.", updateKeyError: "Unable to update API key",
    filterLabel: "Filter API keys", currentFilter: "Current", workingFilter: "Working", attentionFilter: "Needs attention", disabledFilter: "Revoked", allFilter: "All",
    noActiveKeys: "No current API keys.", noWorkingKeys: "No working API keys.", noAttentionKeys: "No keys need attention.", noDisabledKeys: "No revoked API keys.", activeStatus: "Working", disabledStatus: "Revoked",
    createKey: "Create key", createKeyTitle: "Create an API key", createKeyHelp: "Add optional guardrails now. The secret is shown only once.",
    keyName: "Key name", keyNameHint: "For example, Production or CI", keyNameHelp: "Use the environment or tool name so this credential stays recognizable.", guardrailsTitle: "Usage guardrails", guardrailsHelp: "Optional limits protect a leaked or forgotten credential without affecting your other keys.", spendLimit: "Spending limit", spendLimitHint: "Lifetime platform spend cap in USD", optional: "Optional", expiration: "Expiration date", noExpiration: "Never expires", expirationHint: "Expires at the end of this day in your local time.",
    cancel: "Cancel", creating: "Creating…", invalidSpendLimit: "Enter a positive USD amount with up to 2 decimals.", invalidExpiration: "Choose a future expiration date.",
    committedSpend: "Billed and reserved", policyLimitHint: "Leave empty for unlimited. Up to 9 decimal places.", policyExpirationHint: "Leave empty to keep this key from expiring.",
    savePolicy: "Save changes", savingPolicy: "Saving…", invalidPolicySpendLimit: "Enter a positive USD amount with up to 9 decimals.",
    policyBelowCommitted: "The limit cannot be below billed and reserved usage ({amount}).", policyReactivates: "Increasing or removing this guardrail can make the key usable immediately.",
    searchKeys: "Search by name or key suffix", sortBy: "Sort by", sortNewest: "Newest", sortName: "Name", sortSpend: "Highest spend", sortLastUsed: "Recently billed",
    keyHealthSummary: "API key health summary", usableNow: "Working now", usableNowHelp: "Can make requests", blockedNow: "Blocked", blockedNowHelp: "Expired or at limit", watchlist: "Watchlist", watchlistHelp: "Near a guardrail", totalKeySpend: "Total key spend", totalKeySpendHelp: "Lifetime billed usage",
    keysListTitle: "Your API keys", keysListSummary: "Showing {shown} of {total} keys",
    colName: "Integration", colKey: "Credential", colLastUsed: "Last billed", colSpend: "Usage", colLimit: "Limit", colExpires: "Expires", colStatus: "Status", colActions: "Actions",
    spentOfLimit: "{spent} of {limit}", spentWithoutLimit: "{spent} spent · no limit", createdOn: "Created {date}", createFirstKey: "Create your first key", clearSearch: "Clear search", viewCurrentKeys: "View current keys",
    never: "Never", neverUsed: "No billed usage", unlimited: "Unlimited", expiredStatus: "Expired", limitReachedStatus: "Limit reached", expiresSoonStatus: "Expires soon", nearLimitStatus: "Near limit", moreActions: "More actions", openDocs: "Integration guide", revokeKey: "Revoke key",
    revokeTitle: "Revoke this key?", revokeBody: "Requests using this key will stop immediately. This action cannot be undone.", confirmRevoke: "Revoke key", noSearchResults: "No API keys match your search.",
    partialLedger: "Showing only the latest 100 ledger entries. Usage, key, transaction, and top-up totals based on this list may be incomplete.",
    topupNextRemaining: "Add {amount} more to reach {tier} (−{discount}%).",
    payWith: "Payment method",
  },
  ru: {
    logoutError: "Не удалось выйти. Серверная сессия всё ещё активна; повторите попытку.", loggingOut: "Выходим…",
    invalidCheckoutUrl: "Платёжный сервис вернул небезопасный адрес. Страница оплаты не была открыта.",
    invalidWholeUsd: "Введите целую положительную сумму в USD только цифрами: без дробей, знаков, разделителей и ведущих нулей.",
    editKey: "Изменить", editKeyTitle: "Изменить API-ключ", editKeyHelp: "Измените название, лимит расходов или срок действия. Ограничения сразу применяются к новым запросам.", labelRequired: "Введите название перед сохранением.", updateKeyError: "Не удалось изменить API-ключ",
    filterLabel: "Фильтр API-ключей", currentFilter: "Текущие", workingFilter: "Работают", attentionFilter: "Требуют внимания", disabledFilter: "Отозваны", allFilter: "Все",
    noActiveKeys: "Текущих API-ключей нет.", noWorkingKeys: "Работающих API-ключей нет.", noAttentionKeys: "Нет ключей, требующих внимания.", noDisabledKeys: "Отозванных API-ключей нет.", activeStatus: "Работает", disabledStatus: "Отозван",
    createKey: "Создать ключ", createKeyTitle: "Создать API-ключ", createKeyHelp: "При необходимости задайте ограничения. Секрет будет показан только один раз.",
    keyName: "Название ключа", keyNameHint: "Например, Production или CI", keyNameHelp: "Укажите среду или инструмент, чтобы потом легко узнать этот ключ.", guardrailsTitle: "Ограничения использования", guardrailsHelp: "Необязательные ограничения защищают забытый или утёкший ключ, не затрагивая остальные.", spendLimit: "Лимит расходов", spendLimitHint: "Общий лимит расходов платформы в USD", optional: "Необязательно", expiration: "Дата истечения", noExpiration: "Без срока", expirationHint: "Ключ истечёт в конце выбранного дня по вашему местному времени.",
    cancel: "Отмена", creating: "Создаём…", invalidSpendLimit: "Введите положительную сумму USD максимум с 2 знаками после запятой.", invalidExpiration: "Выберите будущую дату истечения.",
    committedSpend: "Списано и зарезервировано", policyLimitHint: "Оставьте пустым, чтобы убрать лимит. До 9 знаков после запятой.", policyExpirationHint: "Оставьте пустым, чтобы ключ не истекал.",
    savePolicy: "Сохранить изменения", savingPolicy: "Сохраняем…", invalidPolicySpendLimit: "Введите положительную сумму USD максимум с 9 знаками после запятой.",
    policyBelowCommitted: "Лимит не может быть меньше уже списанной и зарезервированной суммы ({amount}).", policyReactivates: "Повышение или снятие ограничения может сразу снова активировать ключ.",
    searchKeys: "Поиск по названию или окончанию ключа", sortBy: "Сортировка", sortNewest: "Сначала новые", sortName: "По названию", sortSpend: "По расходам", sortLastUsed: "Недавние списания",
    keyHealthSummary: "Сводка состояния API-ключей", usableNow: "Работают", usableNowHelp: "Могут отправлять запросы", blockedNow: "Заблокированы", blockedNowHelp: "Истекли или исчерпали лимит", watchlist: "Под наблюдением", watchlistHelp: "Близки к ограничению", totalKeySpend: "Расход по ключам", totalKeySpendHelp: "За всё время",
    keysListTitle: "Ваши API-ключи", keysListSummary: "Показано {shown} из {total}",
    colName: "Интеграция", colKey: "Учётные данные", colLastUsed: "Последнее списание", colSpend: "Использование", colLimit: "Лимит", colExpires: "Истекает", colStatus: "Статус", colActions: "Действия",
    spentOfLimit: "{spent} из {limit}", spentWithoutLimit: "Потрачено {spent} · без лимита", createdOn: "Создан {date}", createFirstKey: "Создать первый ключ", clearSearch: "Очистить поиск", viewCurrentKeys: "Показать текущие ключи",
    never: "Никогда", neverUsed: "Списаний не было", unlimited: "Без лимита", expiredStatus: "Истёк", limitReachedStatus: "Лимит исчерпан", expiresSoonStatus: "Скоро истечёт", nearLimitStatus: "Лимит близко", moreActions: "Другие действия", openDocs: "Инструкция подключения", revokeKey: "Отозвать ключ",
    revokeTitle: "Отозвать этот ключ?", revokeBody: "Запросы с этим ключом сразу перестанут работать. Действие нельзя отменить.", confirmRevoke: "Отозвать ключ", noSearchResults: "По вашему запросу ключи не найдены.",
    partialLedger: "Показаны только последние 100 записей журнала. Итоги использования, ключей, операций и пополнений по этому списку могут быть неполными.",
    topupNextRemaining: "Добавьте ещё {amount}, чтобы получить {tier} (−{discount}%).",
    payWith: "Способ оплаты",
  },
} as const;

const navigation: Array<{ section?: Section; label: keyof DashboardCopy; icon: string; href?: string; group?: keyof DashboardCopy }> = [
  { group: "navStart", section: "overview", label: "navOverview", icon: "▦" },
  { group: "navDevelopers", section: "keys", label: "navKeys", icon: "⚿" },
  { href: DOCS_URL, label: "navDocs", icon: "↗" },
  { group: "navBilling", section: "credits", label: "navTopUp", icon: "＋" },
  { group: "navGrowth", section: "promos", label: "navPromos", icon: "%" },
  { group: "navActivity", section: "usage", label: "navUsage", icon: "◔" },
  { group: "navSupportGroup", section: "support", label: "navSupport", icon: "☏" },
  { group: "navAccount", section: "profile", label: "navProfile", icon: "◍" },
];

function useDashboardCopy(): DashboardCopy {
  const { language } = useI18n();
  return dashboardCopy[language];
}

export function Dashboard() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const { language, setLanguage } = useI18n();
  const copy = dashboardCopy[language];
  const localCopy = localDashboardCopy[language];
  const [section, setSection] = useState<Section>(() => parseDashboardSection(searchParams.get("view")));
  const [policyNow] = useState(() => Date.now());
  const [user, setUser] = useState<AuthUser | null>(null);
  const [account, setAccount] = useState<AccountView | null>(null);
  const [keys, setKeys] = useState<ApiKeyView[]>([]);
  const [ledger, setLedger] = useState<LedgerEntry[]>([]);
  const [usage, setUsage] = useState<UsageView | null>(null);
  const [dataErrors, setDataErrors] = useState<Partial<Record<OptionalDataSource, true>>>({});
  const [dataPending, setDataPending] = useState<Record<OptionalDataSource, boolean>>({ keys: true, ledger: true, usage: true });
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [logoutError, setLogoutError] = useState<string | null>(null);
  const [loggingOut, setLoggingOut] = useState(false);
  const [sideOpen, setSideOpen] = useState(false);
  const analyticsLoaded = useRef(false);
  const initialSection = useRef(section);
  const lifecycleGeneration = useRef(0);
  const optionalRequestGeneration = useRef<Record<OptionalDataSource, number>>({ keys: 0, ledger: 0, usage: 0 });

  const retryOptional = useCallback(async (source: OptionalDataSource, showPending = true) => {
    const lifecycle = lifecycleGeneration.current;
    const request = ++optionalRequestGeneration.current[source];
    if (showPending) setDataPending((current) => ({ ...current, [source]: true }));
    setDataErrors((current) => {
      const next = { ...current };
      delete next[source];
      return next;
    });
    try {
      if (source === "keys") {
        const result = await api.apiKeys();
        if (lifecycle !== lifecycleGeneration.current || request !== optionalRequestGeneration.current[source]) return;
        setKeys(result.keys);
      } else if (source === "ledger") {
        const result = await api.ledger(100);
        if (lifecycle !== lifecycleGeneration.current || request !== optionalRequestGeneration.current[source]) return;
        setLedger(result.entries);
        if (result.entries.some((entry) => entry.kind === "topup")) trackFirstProductEvent("topup", "First Top Up", { detected_in: "dashboard" });
        if (result.entries.some((entry) => entry.kind === "charge")) trackFirstProductEvent("api_usage", "First API Usage", { detected_in: "dashboard" });
      } else {
        const result = await api.usage("30d");
        if (lifecycle !== lifecycleGeneration.current || request !== optionalRequestGeneration.current[source]) return;
        setUsage(result);
        if (result.requests > 0) trackFirstProductEvent("api_usage", "First API Usage", { detected_in: "dashboard" });
      }
    } catch {
      // A post-mutation background refresh must not unmount the key manager:
      // doing so would discard a newly issued secret that can only be shown once.
      if (showPending && lifecycle === lifecycleGeneration.current && request === optionalRequestGeneration.current[source]) {
        setDataErrors((current) => ({ ...current, [source]: true }));
      }
    } finally {
      if (showPending && lifecycle === lifecycleGeneration.current && request === optionalRequestGeneration.current[source]) {
        setDataPending((current) => ({ ...current, [source]: false }));
      }
    }
  }, []);

  const load = useCallback(async () => {
    const lifecycle = ++lifecycleGeneration.current;
    setLoading(true);
    setError(null);
    try {
      const [identity, accountView] = await Promise.all([api.me(), api.account()]);
      if (lifecycle !== lifecycleGeneration.current) return;
      const { user: current } = identity;
      setUser(current); setAccount(accountView);
      setLoading(false);
      if (!analyticsLoaded.current) {
        analyticsLoaded.current = true;
        trackProductEvent("Dashboard Opened", { section: initialSection.current, customer_type: current.customerType });
        trackFirstProductEvent("dashboard", "First Dashboard Open", { customer_type: current.customerType });
      }
      // Optional sections hydrate independently after the account shell is ready.
      void Promise.all([retryOptional("keys"), retryOptional("ledger"), retryOptional("usage")]);
    } catch (cause) {
      if (lifecycle !== lifecycleGeneration.current) return;
      if (cause instanceof ApiError && cause.status === 401) { router.replace("/login"); return; }
      setError(cause instanceof Error ? cause.message : dashboardCopy.en.loadError);
    } finally {
      if (lifecycle === lifecycleGeneration.current) setLoading(false);
    }
  }, [retryOptional, router]);

  useEffect(() => {
    document.body.classList.add("app-body");
    const timer = window.setTimeout(() => { void load(); }, 0);
    return () => { lifecycleGeneration.current += 1; window.clearTimeout(timer); document.body.classList.remove("app-body"); };
  }, [load]);

  useEffect(() => {
    function syncSectionFromHistory() {
      setSection(parseDashboardSection(new URLSearchParams(window.location.search).get("view")));
    }
    window.addEventListener("popstate", syncSectionFromHistory);
    return () => window.removeEventListener("popstate", syncSectionFromHistory);
  }, []);

  // Тихо переподтягиваем аккаунт при возврате фокуса: партнёрская скидка-«пол» реферала обычно
  // применяется синхронно при регистрации, но если она доехала async-фидом уже после открытия
  // дашборда — так витрина (панель «Партнёрская ставка») обновится без ручной перезагрузки.
  useEffect(() => {
    let cancelled = false;
    async function refreshAccount() {
      if (document.visibilityState !== "visible") return;
      try {
        const fresh = await api.account();
        if (!cancelled) setAccount(fresh);
      } catch {
        // тихо: это лишь фоновое обновление, ошибки уже покрыты основной загрузкой
      }
    }
    document.addEventListener("visibilitychange", refreshAccount);
    window.addEventListener("focus", refreshAccount);
    return () => {
      cancelled = true;
      document.removeEventListener("visibilitychange", refreshAccount);
      window.removeEventListener("focus", refreshAccount);
    };
  }, []);

  useEffect(() => {
    if (searchParams.get("view") === "security") {
      window.history.replaceState(null, "", dashboardHref("profile", language));
    }
  }, [language, searchParams]);

  async function logout() {
    if (loggingOut) return;
    setLoggingOut(true); setLogoutError(null);
    try { await api.logout(); router.replace("/login"); }
    catch { setLogoutError(localCopy.logoutError); }
    finally { setLoggingOut(false); }
  }

  function open(next: Section) {
    setSideOpen(false);
    setSection(next);
    trackProductEvent("Dashboard Section Viewed", { section: next });
    window.history.pushState(null, "", dashboardHref(next, language));
    window.scrollTo({ top: 0, behavior: "auto" });
  }

  if (loading) return <div className="dashboard-loading"><span className="brand">apiToken.sale</span><p>{copy.loading}</p></div>;
  if (!user || !account) return <div className="wrap guard ym-hide-content"><div className="auth-card"><p>{error ?? copy.loginPrompt}</p><Link className="btn btn-primary" href="/login">{copy.login}</Link></div></div>;

  const usableKeys = keys.filter((key) => isApiKeyUsable(key, policyNow));
  const sourceNotices: Array<{ source: OptionalDataSource; message: string; pending: boolean }> = [];
  if (section === "keys") {
    if (dataPending.keys) sourceNotices.push({ source: "keys", message: copy.keysDataLoading, pending: true });
    else if (dataErrors.keys) sourceNotices.push({ source: "keys", message: copy.keysDataUnavailable, pending: false });
  }
  if (section === "credits" || section === "usage" || section === "promos") {
    if (dataPending.ledger) sourceNotices.push({ source: "ledger", message: copy.ledgerDataLoading, pending: true });
    else if (dataErrors.ledger) sourceNotices.push({ source: "ledger", message: copy.ledgerDataUnavailable, pending: false });
  }
  if (section === "usage") {
    if (dataPending.usage) sourceNotices.push({ source: "usage", message: copy.usageDataLoading, pending: true });
    else if (dataErrors.usage) sourceNotices.push({ source: "usage", message: copy.usageDataUnavailable, pending: false });
  }
  return <div className="app ym-hide-content">
    <aside className={`side ${sideOpen ? "open" : ""}`}>
      <Link className="brand side-brand" href="/"><BrandImages />apiToken.sale</Link>
      <nav className="side-nav">
        {navigation.map((item, index) => <div key={`${item.label}-${index}`} className="side-nav-item">
          {item.group && <span className="side-group">{copy[item.group]}</span>}
          {item.href ? <Link className="side-link" href={item.href} target="_blank" rel="noreferrer"><span className="si">{item.icon}</span><span>{copy[item.label]}</span></Link> :
            <button data-dashboard-section={item.section} className={section === item.section ? "on" : ""} aria-current={section === item.section ? "page" : undefined} onClick={() => open(item.section!)}><span className="si">{item.icon}</span><span>{copy[item.label]}</span></button>}
        </div>)}
      </nav>
      <div className="side-foot">
        <div className="side-tools"><div className="lang"><button className={language === "en" ? "active" : ""} onClick={() => setLanguage("en")}>EN</button><button className={language === "ru" ? "active" : ""} onClick={() => setLanguage("ru")}>RU</button></div><ThemeToggle /></div>
        <nav className="side-legal" aria-label={language === "ru" ? "Правовая информация" : "Legal information"}>
          <Link href="/privacy" target="_blank">{language === "ru" ? "Конфиденциальность" : "Privacy"}</Link>
          <Link href="/terms" target="_blank">{language === "ru" ? "Соглашение" : "Agreement"}</Link>
          <Link href="/support" target="_blank">{language === "ru" ? "Поддержка" : "Support"}</Link>
          <Link href="/plans" target="_blank">{language === "ru" ? "Цены" : "Pricing"}</Link>
        </nav>
        <div className="side-user"><span className="side-av">{(user.displayName || user.email)[0]?.toUpperCase()}</span><div className="side-uinfo"><b>{user.displayName || user.email.split("@")[0]}</b><span>{user.email}</span></div></div>
        <button className="btn btn-ghost btn-sm side-logout" disabled={loggingOut} onClick={logout}>{loggingOut ? localCopy.loggingOut : copy.logout}</button>
      </div>
    </aside>
    <button className={`side-scrim ${sideOpen ? "show" : ""}`} onClick={() => setSideOpen(false)} aria-label={copy.closeMenu} />
    <main className="app-main">
      <header className="app-top"><button className="app-burger" onClick={() => setSideOpen(true)} aria-label={copy.menu}>☰</button><div className="app-top-h"><div className="app-title">{copy[navigation.find((item) => item.section === section)?.label ?? "navOverview"]}</div></div>
        <div className="app-top-actions">
          {section === "overview" ? <button className="btn btn-primary btn-sm app-top-up" onClick={() => open("credits")}>{copy.topUp}</button> : <button className="app-top-bal" onClick={() => open("credits")} title={copy.navTopUp}>
            <span className="atb-ic" aria-hidden="true" />
            <span className="atb-label">{copy.creditsLabel}</span>
            <span className={`atb-val${BigInt(account.balanceNano) < 0n ? " atb-neg" : ""}`}>{formatNanoUsd(account.balanceNano)}</span>
          </button>}
        </div>
      </header>
      <div className="app-body-in">
        {error && <div className="banner banner-error">{error} <button className="btn btn-ghost btn-sm" onClick={load}>{copy.retry}</button></div>}
        {logoutError && <div className="banner banner-error">{logoutError} <button className="btn btn-ghost btn-sm" disabled={loggingOut} onClick={logout}>{copy.retry}</button></div>}
        {sourceNotices.map((notice) => <div className={`banner dashboard-data-notice${notice.pending ? "" : " banner-error"}`} role="status" key={notice.source}><span>{notice.message}</span>{!notice.pending && <button className="btn btn-ghost btn-sm" onClick={() => void retryOptional(notice.source)}>{copy.retry}</button>}</div>)}
        {section === "overview" && <Overview
          account={account}
          user={user}
          usableKeys={usableKeys}
          totalKeys={keys.length}
          keysState={dataPending.keys ? "loading" : dataErrors.keys ? "unavailable" : "ready"}
          usage={usage}
          usageState={dataPending.usage ? "loading" : dataErrors.usage ? "unavailable" : "ready"}
          ledger={ledger}
          ledgerState={dataPending.ledger ? "loading" : dataErrors.ledger ? "unavailable" : "ready"}
          open={open}
        />}
        {section === "keys" && !dataPending.keys && !dataErrors.keys && <ApiKeys keys={keys} onChanged={() => retryOptional("keys", false)} user={user} />}
        {section === "credits" && <Credits account={account} ledger={ledger} ledgerAvailable={!dataPending.ledger && !dataErrors.ledger} />}
        {section === "usage" && usage && <Usage account={account} ledger={ledger} usage={usage} ledgerAvailable={!dataPending.ledger && !dataErrors.ledger} />}
        {section === "support" && <SupportPanel />}
        {section === "profile" && <Profile user={user} onUpdated={setUser} />}
        {section === "promos" && <PromoPanel ledger={ledger} ledgerAvailable={!dataPending.ledger && !dataErrors.ledger} ledgerMayBePartial={ledger.length >= 100} />}
      </div>
    </main>
  </div>;
}

function BrandImages() {
  return <><Image className="brand-mark bm-light" src="/assets/logo-mark-light.png" width={24} height={24} alt="" /><Image className="brand-mark bm-dark" src="/assets/logo-mark-dark.png" width={24} height={24} alt="" /></>;
}

function PageHeading({ eyebrow, title, subtitle }: { eyebrow: string; title: string; subtitle: string }) {
  return <header className="page-heading"><span className="eyebrow">{eyebrow}</span><h1 className="p-h1">{title}</h1><p className="p-sub">{subtitle}</p></header>;
}

type OverviewDataState = "loading" | "unavailable" | "ready";

function Overview({ account, user, usableKeys, totalKeys, keysState, usage, usageState, ledger, ledgerState, open }: {
  account: AccountView;
  user: AuthUser;
  usableKeys: ApiKeyView[];
  totalKeys: number;
  keysState: OverviewDataState;
  usage: UsageView | null;
  usageState: OverviewDataState;
  ledger: LedgerEntry[];
  ledgerState: OverviewDataState;
  open(section: Section): void;
}) {
  const copy = useDashboardCopy();
  const { language } = useI18n();
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const multiplierBp = paymentBasisPoints(account);
  const discount = discountOf(account);
  const officialBalanceNano = officialNanoFromCharged(BigInt(account.balanceNano), multiplierBp);
  const engineReady = account.status === "active" && user.engineAccountStatus === "active";
  const keysReady = engineReady && keysState === "ready" && usableKeys.length > 0;
  const accessTone = keysState === "loading" ? "neutral"
    : keysState === "unavailable" || !engineReady || (totalKeys > 0 && !keysReady) ? "warning"
      : keysReady ? "success" : "setup";
  const accessLabel = keysState === "loading" ? copy.checking
    : keysState === "unavailable" ? copy.statusUnavailable
      : !engineReady || totalKeys > 0 && !keysReady ? copy.needsAttention
        : keysReady ? copy.ready : copy.setupRequired;
  const keyStatusText = keysState === "loading" ? copy.checkingKeyStatus
    : keysState === "unavailable" ? copy.keysUnavailable
      : !engineReady ? copy.engineNotReady
        : keysReady ? copy.keysReady
          : totalKeys > 0 ? copy.keysNeedAttention : copy.noKeysOverview;
  const balanceNano = BigInt(account.balanceNano);
  const lowBalance = balanceNano > 0n && balanceNano <= 5n * NANO_PER_USD;
  const recentActivity = ledgerState === "ready"
    ? [...ledger].sort((left, right) => ledgerMs(right.timestamp) - ledgerMs(left.timestamp)).slice(0, 3)
    : [];
  const pricing = account.pricing;
  const progressivePricing = pricing?.customerType === "b2c" && !isPartnerRate(account) ? pricing : null;
  const isProgressive = progressivePricing !== null;
  const pricingTitle = !pricing ? copy.standardPricing
    : pricing.customerType === "b2b" ? copy.businessAgreement
      : isPartnerRate(account) ? copy.partnerRate : tierName(copy, pricing.tier);
  const showOnboarding = engineReady && keysState === "ready" && totalKeys === 0;

  let alert: { tone: "danger" | "warning"; title: string; text: string; action: "credits" | "keys" } | null = null;
  if (!engineReady) alert = { tone: "danger", title: copy.apiAccessBlocked, text: copy.engineNotReady, action: "keys" };
  else if (keysState === "ready" && totalKeys > 0 && usableKeys.length === 0) alert = { tone: "warning", title: copy.keysNeedAttentionTitle, text: copy.keysNeedAttention, action: "keys" };
  else if (balanceNano <= 0n) alert = { tone: "danger", title: copy.balanceEmptyTitle, text: copy.balanceEmptyText, action: "credits" };
  else if (lowBalance) alert = { tone: "warning", title: copy.balanceLowTitle, text: copy.balanceLowText, action: "credits" };

  return <section className="panel overview-panel">
    {alert && <div className={`overview-alert ${alert.tone}`} role="status">
      <span className="overview-alert-icon" aria-hidden="true">!</span>
      <div><strong>{alert.title}</strong><span>{alert.text}</span></div>
      <button className="btn btn-ghost btn-sm" onClick={() => open(alert.action)}>{alert.action === "credits" ? copy.topUp : copy.manageKeys}</button>
    </div>}

    <div className="overview-primary-grid">
      <article className="card overview-balance-card">
        <div className="overview-card-head">
          <span className="overview-card-label">{copy.platformBalance}</span>
          <span className="overview-rate-chip">{discount}% {copy.discount} · {formatMultiplier(multiplierBp)}</span>
        </div>
        <strong className="overview-balance-number">{normalizeUsd(account.balanceUsd)}</strong>
        <p className="overview-balance-value">{copy.worthApproximately} <b>≈ {formatNanoUsd(officialBalanceNano)}</b> {copy.inClaudeApiUsage}</p>
        <p className="overview-balance-rate">{interpolate(copy.payPerOfficialDollar, { rate: formatPaymentRate(multiplierBp) })}</p>
        <div className="overview-card-actions">
          <button className="btn btn-primary btn-sm" onClick={() => open("credits")}>{copy.topUp}</button>
          <button className="btn btn-ghost btn-sm" onClick={() => open("usage")}>{copy.viewUsage}</button>
        </div>
      </article>

      <article className={`card overview-access-card ${accessTone}`}>
        <div className="overview-card-head">
          <span className="overview-card-label">{copy.apiAccess}</span>
          <span className={`overview-status ${accessTone}`}><i aria-hidden="true" />{accessLabel}</span>
        </div>
        <strong className="overview-access-value">{keysState === "ready" ? usableKeys.length : "—"}</strong>
        <span className="overview-access-unit">{copy.usableKeys}</span>
        <p>{keyStatusText}</p>
        <button className="btn btn-ghost btn-sm" onClick={() => open("keys")}>{totalKeys > 0 ? copy.manageKeys : copy.getKey}</button>
      </article>
    </div>

    {showOnboarding && <section className="card overview-onboarding">
      <div className="overview-onboarding-copy">
        <span className="overview-card-label">{copy.quickStart}</span>
        <h2>{copy.startFirstRequest}</h2>
        <p>{copy.startFirstRequestText}</p>
      </div>
      <ol>
        <li><span>1</span><div><strong>{copy.createFirstKey}</strong><small>{copy.createFirstKeyHint}</small></div></li>
        <li><span>2</span><div><strong>{copy.connectYourTool}</strong><small>{copy.connectYourToolHint}</small></div></li>
        <li><span>3</span><div><strong>{copy.makeFirstRequest}</strong><small>{copy.makeFirstRequestHint}</small></div></li>
      </ol>
      <button className="btn btn-primary btn-sm" onClick={() => open("keys")}>{copy.getKey}</button>
    </section>}

    <div className="overview-metrics-grid">
      <article className="card overview-metric-card">
        <div className="overview-card-head"><span className="overview-card-label">{copy.usageLast30Days}</span><span className="overview-metric-mark" aria-hidden="true">↗</span></div>
        <strong>{usageState === "ready" && usage ? formatNanoUsd(usage.totalOfficialNano) : "—"}</strong>
        <p>{usageState === "loading" ? copy.loadingUsageSummary
          : usageState === "unavailable" || !usage ? copy.usageSummaryUnavailable
            : interpolate(copy.usageChargedAndRequests, { charged: formatNanoUsd(usage.totalChargedNano), requests: usage.requests.toLocaleString(locale) })}</p>
        <button className="link plain-button overview-card-link" onClick={() => open("usage")}>{copy.viewUsage} →</button>
      </article>

      <article className="card overview-metric-card overview-pricing-card">
        <div className="overview-card-head"><span className="overview-card-label">{copy.currentPricing}</span><span className="overview-metric-mark" aria-hidden="true">%</span></div>
        <strong>{pricingTitle}</strong>
        <p><b>{discount}% {copy.discount}</b> · {formatMultiplier(multiplierBp)} {copy.valueMultiplier}</p>
        <span className="overview-pricing-foot">{interpolate(copy.payPerOfficialDollar, { rate: formatPaymentRate(multiplierBp) })}</span>
      </article>

      <article className="card overview-metric-card overview-milestone-card">
        {progressivePricing?.nextTier ? <>
          <div className="overview-card-head"><span className="overview-card-label">{copy.nextMilestone}</span><span className="overview-metric-mark" aria-hidden="true">→</span></div>
          <strong>{tierName(copy, progressivePricing.nextTier.tier)} · {progressivePricing.nextTier.discountPercent}%</strong>
          <p>{interpolate(copy.remainingToUnlock, { amount: formatNanoUsd(progressivePricing.nextTier.remainingNano) })}</p>
          <div className="overview-progress" role="progressbar" aria-label={copy.tierProgressLabel} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(boundedPercent(BigInt(progressivePricing.spentNano), BigInt(progressivePricing.nextTier.spendThresholdNano)))}>
            <span style={{ width: `${boundedPercent(BigInt(progressivePricing.spentNano), BigInt(progressivePricing.nextTier.spendThresholdNano))}%` }} />
          </div>
          <small>{interpolate(copy.topupsTowardTier, { current: formatNanoUsd(progressivePricing.spentNano), target: formatNanoUsd(progressivePricing.nextTier.spendThresholdNano) })}</small>
        </> : <>
          <div className="overview-card-head"><span className="overview-card-label">{isProgressive ? copy.milestonesComplete : copy.pricingTerms}</span><span className="overview-metric-mark" aria-hidden="true">✓</span></div>
          <strong>{isProgressive ? copy.highestTierReached : copy.fixedRate}</strong>
          <p>{isProgressive ? copy.highestTierSummary : copy.fixedRateSummary}</p>
        </>}
        <Link className="link overview-card-link" href={`${DOCS_URL}#pricing`}>{copy.howTiersWork} →</Link>
      </article>
    </div>

    <section className="card overview-activity">
      <div className="overview-activity-head">
        <div><span className="overview-card-label">{copy.recentActivity}</span><h2>{copy.latestAccountActivity}</h2></div>
        <button className="link plain-button overview-card-link" onClick={() => open("usage")}>{copy.viewAllActivity} →</button>
      </div>
      {ledgerState === "loading" ? <div className="overview-activity-empty">{copy.loadingRecentActivity}</div>
        : ledgerState === "unavailable" ? <div className="overview-activity-empty">{copy.recentActivityUnavailable}</div>
          : recentActivity.length === 0 ? <div className="overview-activity-empty">{copy.noLedger}</div>
            : <div className="overview-activity-list">{recentActivity.map((entry) => {
              const amount = BigInt(entry.amountNano);
              const isCharge = entry.kind === "charge";
              const isTopup = entry.kind === "topup";
              const activityLabel = isCharge ? copy.apiUsageActivity : isTopup ? copy.topupType : copy.adjustType;
              const activityDetail = entry.model ? modelLabel(entry.model) : entry.keyMasked ?? entry.reference ?? copy.accountAdjustment;
              const amountPrefix = isCharge ? "−" : amount > 0n ? "+" : "";
              return <div className="overview-activity-row" key={entry.id}>
                <span className={`overview-activity-icon ${entry.kind}`} aria-hidden="true">{isCharge ? "↗" : isTopup ? "+" : "±"}</span>
                <div className="overview-activity-name"><strong>{activityLabel}</strong><span>{activityDetail}</span></div>
                <time dateTime={new Date(ledgerMs(entry.timestamp)).toISOString()}>{formatOverviewActivityTime(entry.timestamp, language)}</time>
                <b className={isCharge ? "charge" : isTopup ? "topup" : ""}>{amountPrefix}{formatNanoUsd(absoluteBigInt(amount))}</b>
              </div>;
            })}</div>}
    </section>
  </section>;
}

function Stat({ label, value, detail, onClick }: { label: string; value: string; detail: string; onClick?: () => void }) { return <div className="ovstat"><span className="dlabel">{label}</span><b className="num">{value}</b>{onClick ? <button className="dtrend link plain-button" onClick={onClick}>{detail}</button> : <span className="dtrend">{detail}</span>}</div>; }

function PricingBanner({ account }: { account: AccountView }) {
  const copy = useDashboardCopy();
  // Read "now" once at mount — Date.now() in render is impure (see the same pattern in Usage).
  const [now] = useState(() => Date.now());
  const pricing = account.pricing;
  if (!pricing) return null;
  if (pricing.customerType === "b2b") return <section className="pricing-banner pricing-banner-business"><div className="pricing-summary"><div><span className="pricing-kicker">{copy.currentPricing}</span><strong>{copy.businessAgreement}</strong></div><div className="pricing-discount"><b>{pricing.discountPercent}%</b><span>{copy.discount}</span><em className="pricing-mult">{multFromDiscount(pricing.discountPercent)} {copy.valueMultiplier}</em></div></div><p>{copy.negotiatedRate}</p></section>;
  // Партнёрская фиксированная ставка (реф-ссылка сейлза). Реферал остаётся b2c, но платит по «полу»
  // скидки, а не по прогрессивным тирам — прячем лестницу/удержание, показываем фикс-ставку.
  if (isPartnerRate(account)) {
    const paymentBp = paymentBasisPoints(account);
    const discount = discountOf(account);
    const exampleNano = officialNanoFromCharged(100n * NANO_PER_USD, paymentBp);
    return <section className="pricing-banner pricing-banner-business pricing-banner-partner">
      <div className="pricing-summary">
        <div><span className="pricing-kicker">{copy.partnerRateKicker}</span><strong>{copy.partnerRate}</strong></div>
        <div className="pricing-discount"><b>{discount}%</b><span>{copy.discount}</span><em className="pricing-mult">{formatMultiplier(paymentBp)} {copy.valueMultiplier}</em></div>
      </div>
      <div className="pricing-partner-facts">
        <div className="pricing-status-item"><span>{copy.partnerYouPay}</span><strong>{`$${formatFixedRatio(paymentBp, BASIS_POINTS, 2)}`}</strong><small>{copy.partnerYouPayHint}</small></div>
        <div className="pricing-status-item"><span>{copy.partnerExample}</span><strong>$100 → ≈ {formatNanoUsd(exampleNano)}</strong><small>{copy.partnerExampleHint}</small></div>
        <div className="pricing-status-item pricing-status-ok"><span>{copy.partnerFixed}</span><strong>{copy.partnerFixedValue}</strong><small>{copy.partnerFixedHint}</small></div>
      </div>
      <p>{copy.partnerExplainer}</p>
    </section>;
  }
  const currentIndex = Math.max(0, B2C_PRICING_MILESTONES.findIndex((tier) => tier.code === pricing.tier));
  const currentTier = B2C_PRICING_MILESTONES[currentIndex]!;
  const progress = pricingMilestoneProgress(pricing.tier, pricing.spentNano);
  const trackStyle = { "--tier-progress": `${progress}%` } as CSSProperties;
  const isBase = currentTier.code === "starter";
  const holdNano = BigInt(pricing.retentionSpendNano);
  const windowSpent = BigInt(pricing.windowSpentNano ?? "0");
  const held = windowSpent >= holdNano;
  const daysLeft = pricing.windowStart ? Math.max(0, Math.ceil(30 - (now - new Date(pricing.windowStart).getTime()) / 86_400_000)) : 30;
  return <section className="pricing-banner pricing-banner-milestones">
    <div className="pricing-summary">
      <div><span className="pricing-kicker">{copy.monthlyTierProgress}</span><strong>{tierName(copy, currentTier.code)}</strong></div>
      <div className="pricing-discount"><b>{pricing.discountPercent}%</b><span>{copy.discount}</span><em className="pricing-mult">{multFromDiscount(pricing.discountPercent)} {copy.valueMultiplier}</em></div>
    </div>
    <div className="pricing-milestone-status">
      <div className="pricing-status-item"><span>{copy.thisMonth}</span><strong>{formatNanoUsd(pricing.spentNano)}</strong><small>{copy.platformSpend}</small></div>
      {pricing.nextTier ? <div className="pricing-status-item pricing-status-next"><span>{copy.nextMilestone}</span><strong>{interpolate(copy.spendMore, { amount: formatNanoUsd(pricing.nextTier.remainingNano) })}</strong><small>{interpolate(copy.unlockTier, { tier: tierName(copy, pricing.nextTier.tier), discount: pricing.nextTier.discountPercent })}</small></div> :
        <div className="pricing-status-item pricing-status-next"><span>{copy.milestonesComplete}</span><strong>{copy.highestTierReached}</strong><small>{copy.tierScale} · {pricing.discountPercent}% {copy.discount}</small></div>}
      {isBase
        ? <div className="pricing-status-item"><span>{copy.keepTier}</span><strong>{copy.baseTierKept}</strong><small>{copy.freeForever}</small></div>
        : <div className={`pricing-status-item ${held ? "pricing-status-ok" : "pricing-status-warn"}`}><span>{copy.keepTier}</span><strong>{formatNanoUsd(pricing.windowSpentNano ?? "0")} / {formatNanoUsd(pricing.retentionSpendNano)}</strong><small>{interpolate(held ? copy.daysLeftOk : copy.daysLeftWarn, { days: daysLeft })}</small></div>}
    </div>
    <div className="pricing-milestone-track" style={trackStyle} aria-label={`${Math.round(progress)}% progress through pricing milestones`}>
      <div className="pricing-track-line" aria-hidden="true"><span /></div>
      <ol className="pricing-milestone-list">
        {B2C_PRICING_MILESTONES.map((tier, index) => {
          const state = index < currentIndex ? "complete" : index === currentIndex ? "current" : "upcoming";
          return <li className={`pricing-milestone ${state}`} key={tier.code}>
            <span className="pricing-milestone-dot" aria-hidden="true">{index < currentIndex ? "✓" : index + 1}</span>
            <div><strong>{tierName(copy, tier.code)}</strong><span>{tier.discountPercent}% {copy.discount}</span><em className="pm-mult">{multFromDiscount(tier.discountPercent)} {copy.valueMultiplier}</em><small>{BigInt(tier.platformSpendUsd) === 0n ? copy.tierBaseHint : interpolate(copy.tierGetHold, { get: formatWholeUsd(tier.platformSpendUsd), hold: formatWholeUsd(tier.holdUsd) })}</small></div>
          </li>;
        })}
      </ol>
    </div>
    <div className="pricing-howto-row"><p className="pricing-howto-text">{copy.tierExplainer}</p><Link className="link pricing-howto-link" href={`${DOCS_URL}#pricing`}>{copy.howTiersWork} →</Link></div>
  </section>;
}

function ApiKeys({ keys, onChanged, user }: { keys: ApiKeyView[]; onChanged(): Promise<void>; user: AuthUser }) {
  const copy = useDashboardCopy();
  const { language } = useI18n();
  const localCopy = localDashboardCopy[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const [issued, setIssued] = useState<string | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [label, setLabel] = useState("");
  const [spendLimit, setSpendLimit] = useState("");
  const [expirationDate, setExpirationDate] = useState("");
  const [totpCode, setTotpCode] = useState("");
  const [filter, setFilter] = useState<KeyStatusFilter>("current");
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState<"newest" | "name" | "spend" | "last-used">("newest");
  const [editTarget, setEditTarget] = useState<ApiKeyView | null>(null);
  const [editLabel, setEditLabel] = useState("");
  const [policySpendLimit, setPolicySpendLimit] = useState("");
  const [policyExpirationDate, setPolicyExpirationDate] = useState("");
  const [policyTotpCode, setPolicyTotpCode] = useState("");
  const [revokeTarget, setRevokeTarget] = useState<ApiKeyView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [policyNow, setPolicyNow] = useState(() => Date.now());
  const createTriggerRef = useRef<HTMLButtonElement>(null);
  const createModalRef = useRef<HTMLFormElement>(null);
  const editModalRef = useRef<HTMLFormElement>(null);
  const revokeModalRef = useRef<HTMLDivElement>(null);
  const keysPanelRef = useRef<HTMLElement>(null);
  const dialogReturnFocusRef = useRef<HTMLElement | null>(null);
  const busyRef = useRef(busy);

  useEffect(() => {
    const interval = window.setInterval(() => setPolicyNow(Date.now()), 60_000);
    return () => window.clearInterval(interval);
  }, []);

  useEffect(() => {
    const panel = keysPanelRef.current;
    if (!panel) return;

    const closeMenusExcept = (currentMenu: HTMLDetailsElement | null = null) => {
      panel.querySelectorAll<HTMLDetailsElement>(".key-menu[open]").forEach((menu) => {
        if (menu !== currentMenu) menu.removeAttribute("open");
      });
    };
    const handlePointerDown = (event: PointerEvent) => {
      const target = event.target instanceof Element ? event.target : null;
      const menu = target?.closest(".key-menu");
      closeMenusExcept(menu instanceof HTMLDetailsElement ? menu : null);
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      const menu = panel.querySelector<HTMLDetailsElement>(".key-menu[open]");
      if (!menu) return;
      event.preventDefault();
      menu.removeAttribute("open");
      menu.querySelector<HTMLElement>("summary")?.focus();
    };

    document.addEventListener("pointerdown", handlePointerDown);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("pointerdown", handlePointerDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, []);

  useEffect(() => { busyRef.current = busy; }, [busy]);

  useEffect(() => {
    const modal = createOpen ? createModalRef.current : editTarget ? editModalRef.current : revokeTarget ? revokeModalRef.current : null;
    if (!modal) return;
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    const focusableSelector = "button:not([disabled]),a[href],input:not([disabled]),select:not([disabled]),[tabindex]:not([tabindex='-1'])";
    const focusFirst = () => (modal.querySelector<HTMLElement>("[autofocus]") ?? modal.querySelector<HTMLElement>(focusableSelector) ?? modal).focus();
    const frame = window.requestAnimationFrame(focusFirst);
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        if (busyRef.current) return;
        setError(null);
        if (createOpen) {
          setCreateOpen(false);
          window.requestAnimationFrame(() => createTriggerRef.current?.focus());
        } else {
          if (editTarget) {
            setEditTarget(null); setEditLabel(""); setPolicySpendLimit(""); setPolicyExpirationDate(""); setPolicyTotpCode("");
          } else {
            setRevokeTarget(null);
          }
          const returnTarget = dialogReturnFocusRef.current;
          window.requestAnimationFrame(() => returnTarget?.focus());
        }
        return;
      }
      if (event.key !== "Tab") return;
      const focusable = [...modal.querySelectorAll<HTMLElement>(focusableSelector)];
      if (focusable.length === 0) { event.preventDefault(); modal.focus(); return; }
      const first = focusable[0]!, last = focusable.at(-1)!;
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      window.cancelAnimationFrame(frame);
      document.removeEventListener("keydown", onKeyDown);
      document.body.style.overflow = previousOverflow;
    };
  }, [createOpen, editTarget, revokeTarget]);

  function closeCreate() {
    if (busy) return;
    setCreateOpen(false); setError(null);
    window.requestAnimationFrame(() => createTriggerRef.current?.focus());
  }

  function openEdit(key: ApiKeyView, returnTarget?: HTMLElement | null) {
    keysPanelRef.current?.querySelectorAll<HTMLDetailsElement>(".key-menu[open]").forEach((menu) => menu.removeAttribute("open"));
    dialogReturnFocusRef.current = returnTarget ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null);
    setEditLabel(key.label ?? "");
    setPolicySpendLimit(key.spendLimitNano ? nanoToUsdInput(key.spendLimitNano) : "");
    setPolicyExpirationDate(key.expiresAt ? isoToLocalDateInput(key.expiresAt) : "");
    setPolicyTotpCode(""); setError(null); setEditTarget(key);
  }

  function closeEdit() {
    if (busy) return;
    setEditTarget(null); setEditLabel(""); setPolicySpendLimit(""); setPolicyExpirationDate(""); setPolicyTotpCode(""); setError(null);
    const returnTarget = dialogReturnFocusRef.current;
    window.requestAnimationFrame(() => returnTarget?.focus());
  }

  function openRevoke(key: ApiKeyView, returnTarget?: HTMLElement | null) {
    dialogReturnFocusRef.current = returnTarget ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null);
    setError(null); setRevokeTarget(key);
  }

  function closeRevoke() {
    if (busy) return;
    setRevokeTarget(null); setError(null);
    const returnTarget = dialogReturnFocusRef.current;
    window.requestAnimationFrame(() => returnTarget?.focus());
  }

  async function create(event: FormEvent) {
    event.preventDefault();
    if (user.totpEnabled && !/^\d{6}$/.test(totpCode)) { setError(copy.twoFactorCodeRequired); return; }
    const trimmedLimit = spendLimit.trim();
    if (trimmedLimit &&
        (!/^(?:0\.\d{1,2}|[1-9]\d*(?:\.\d{1,2})?)$/.test(trimmedLimit) || Number(trimmedLimit) <= 0)) {
      setError(localCopy.invalidSpendLimit); return;
    }
    let expiresAt: string | undefined;
    if (expirationDate) {
      const date = new Date(`${expirationDate}T23:59:59.999`);
      if (!Number.isFinite(date.getTime()) || date.getTime() <= Date.now()) {
        setError(localCopy.invalidExpiration); return;
      }
      expiresAt = date.toISOString();
    }
    setBusy(true); setError(null);
    try {
      const created = await api.createApiKey({
        ...(label.trim() ? { label: label.trim() } : {}),
        ...(trimmedLimit ? { spendLimitUsd: trimmedLimit } : {}),
        ...(expiresAt ? { expiresAt } : {}),
        ...(user.totpEnabled ? { totpCode } : {}),
      });
      trackProductEvent("API Key Created", {
        has_label: Boolean(label.trim()), has_limit: Boolean(trimmedLimit), has_expiration: Boolean(expiresAt),
        two_factor: user.totpEnabled,
      });
      trackFirstProductEvent("api_key", "First API Key Created", { two_factor: user.totpEnabled });
      setIssued(created.key ?? null);
      setLabel(""); setSpendLimit(""); setExpirationDate(""); setTotpCode(""); setCreateOpen(false);
      await onChanged();
    } catch (cause) {
      const message = cause instanceof ApiError && (cause.message === "2fa_required" || cause.message === "2fa_invalid")
        ? copy.twoFactorCodeInvalid
        : cause instanceof Error ? cause.message : copy.createKeyError;
      setError(message);
    } finally { setBusy(false); }
  }

  async function updateKey(event: FormEvent) {
    event.preventDefault();
    if (!editTarget) return;
    const nextLabel = editLabel.trim();
    const currentLabel = (editTarget.label ?? "").trim();
    const labelChanged = nextLabel !== currentLabel;
    const guardrailsChanged = editTarget.status === "active" && (
      policySpendLimit.trim() !== (editTarget.spendLimitNano ? nanoToUsdInput(editTarget.spendLimitNano) : "") ||
      policyExpirationDate !== (editTarget.expiresAt ? isoToLocalDateInput(editTarget.expiresAt) : "")
    );
    if (labelChanged && !nextLabel) {
      setError(localCopy.labelRequired); return;
    }
    if (guardrailsChanged && user.totpEnabled && !/^\d{6}$/.test(policyTotpCode)) {
      setError(copy.twoFactorCodeRequired); return;
    }
    const trimmedLimit = policySpendLimit.trim();
    if (guardrailsChanged && trimmedLimit && !/^(?:0\.\d{1,9}|[1-9]\d*(?:\.\d{1,9})?)$/.test(trimmedLimit)) {
      setError(localCopy.invalidPolicySpendLimit); return;
    }
    const proposedNano = trimmedLimit ? usdInputToNano(trimmedLimit) : null;
    if (guardrailsChanged && proposedNano !== null && proposedNano <= 0n) {
      setError(localCopy.invalidPolicySpendLimit); return;
    }
    const committedNano = BigInt(editTarget.spentNano) + BigInt(editTarget.reservedNano ?? "0");
    if (guardrailsChanged && proposedNano !== null && proposedNano < committedNano) {
      setError(interpolate(localCopy.policyBelowCommitted, { amount: formatNanoUsd(committedNano.toString()) }));
      return;
    }
    let expiresAt: string | null = null;
    if (guardrailsChanged && policyExpirationDate) {
      const date = new Date(`${policyExpirationDate}T23:59:59.999`);
      if (!Number.isFinite(date.getTime()) || date.getTime() <= Date.now()) {
        setError(localCopy.invalidExpiration); return;
      }
      expiresAt = date.toISOString();
    }
    setBusy(true); setError(null);
    try {
      if (guardrailsChanged) {
        await api.updateApiKeyPolicy(editTarget.id, {
          spendLimitUsd: trimmedLimit || null,
          expiresAt,
          ...(user.totpEnabled ? { totpCode: policyTotpCode } : {}),
        });
        trackProductEvent("API Key Policy Updated", {
          limit: trimmedLimit ? "set" : "cleared",
          expiration: expiresAt ? "set" : "cleared",
          two_factor: user.totpEnabled,
        });
      }
      if (labelChanged) {
        await api.renameApiKey(editTarget.id, nextLabel);
        trackProductEvent("API Key Renamed");
      }
      await onChanged();
      setEditTarget(null); setEditLabel(""); setPolicySpendLimit(""); setPolicyExpirationDate(""); setPolicyTotpCode("");
      const returnTarget = dialogReturnFocusRef.current;
      window.requestAnimationFrame(() => returnTarget?.focus());
    } catch (cause) {
      const message = cause instanceof ApiError && (cause.message === "2fa_required" || cause.message === "2fa_invalid")
        ? copy.twoFactorCodeInvalid
        : cause instanceof ApiError && cause.status === 409
          ? interpolate(localCopy.policyBelowCommitted, { amount: formatNanoUsd(committedNano.toString()) })
          : cause instanceof Error ? cause.message : localCopy.updateKeyError;
      setError(message);
    } finally { setBusy(false); }
  }

  async function revoke() {
    if (!revokeTarget) return;
    setBusy(true); setError(null);
    try {
      await api.revokeApiKey(revokeTarget.id); trackProductEvent("API Key Revoked");
      setRevokeTarget(null); await onChanged();
      const returnTarget = dialogReturnFocusRef.current;
      window.requestAnimationFrame(() => returnTarget?.focus());
    } catch (cause) { setError(cause instanceof Error ? cause.message : copy.revokeKeyError); }
    finally { setBusy(false); }
  }

  const matchesFilter = (key: ApiKeyView, selectedFilter: KeyStatusFilter) => {
    const policy = keyPolicy(key, policyNow);
    if (selectedFilter === "current") return key.status === "active";
    if (selectedFilter === "working") return key.status === "active" && !policy.expired && !policy.limitReached;
    if (selectedFilter === "attention") return key.status === "active" && (policy.expired || policy.expiresSoon || policy.limitReached || policy.nearLimit);
    if (selectedFilter === "disabled") return key.status === "disabled";
    return true;
  };
  const counts: Record<KeyStatusFilter, number> = {
    current: keys.filter((key) => matchesFilter(key, "current")).length,
    working: keys.filter((key) => matchesFilter(key, "working")).length,
    attention: keys.filter((key) => matchesFilter(key, "attention")).length,
    disabled: keys.filter((key) => matchesFilter(key, "disabled")).length,
    all: keys.length,
  };
  const query = search.trim().toLocaleLowerCase(locale);
  const sortedKeys = [...keys]
    .filter((key) => matchesFilter(key, filter))
    .filter((key) => !query || (key.label ?? copy.unlabelledKey).toLocaleLowerCase(locale).includes(query) || key.keyMasked.toLocaleLowerCase(locale).includes(query))
    .sort((left, right) => {
      if (sort === "name") return (left.label ?? copy.unlabelledKey).localeCompare(right.label ?? copy.unlabelledKey, locale);
      if (sort === "spend") return compareBigInt(BigInt(right.spentNano), BigInt(left.spentNano));
      if (sort === "last-used") return Date.parse(right.lastUsedAt ?? "1970-01-01") - Date.parse(left.lastUsedAt ?? "1970-01-01");
      return Date.parse(right.createdAt) - Date.parse(left.createdAt);
    });
  const emptyMessage = search.trim()
    ? localCopy.noSearchResults
    : filter === "current" ? localCopy.noActiveKeys
      : filter === "working" ? localCopy.noWorkingKeys
        : filter === "attention" ? localCopy.noAttentionKeys
          : filter === "disabled" ? localCopy.noDisabledKeys
            : copy.noKeys;
  const todayDate = new Date(policyNow);
  const today = new Date(todayDate.getTime() - todayDate.getTimezoneOffset() * 60_000).toISOString().slice(0, 10);
  const labelDirty = Boolean(editTarget) && editLabel.trim() !== (editTarget?.label ?? "").trim();
  const policyDirty = editTarget?.status === "active" && (
    policySpendLimit.trim() !== (editTarget.spendLimitNano ? nanoToUsdInput(editTarget.spendLimitNano) : "") ||
    policyExpirationDate !== (editTarget.expiresAt ? isoToLocalDateInput(editTarget.expiresAt) : "")
  );
  const editDirty = labelDirty || policyDirty;
  const editTargetState = editTarget ? keyPolicy(editTarget, policyNow) : null;
  const policyCommittedNano = editTarget
    ? (BigInt(editTarget.spentNano) + BigInt(editTarget.reservedNano ?? "0")).toString()
    : "0";
  const policyStates = keys.map((key) => ({ key, policy: keyPolicy(key, policyNow) }));
  const usableCount = policyStates.filter(({ key, policy }) => key.status === "active" && !policy.expired && !policy.limitReached).length;
  const blockedCount = policyStates.filter(({ key, policy }) => key.status === "active" && (policy.expired || policy.limitReached)).length;
  const watchlistCount = policyStates.filter(({ key, policy }) => key.status === "active" && (policy.expiresSoon || policy.nearLimit)).length;
  const totalKeySpend = keys.reduce((sum, key) => sum + BigInt(key.spentNano), 0n);

  return <section ref={keysPanelRef} className="panel keys-panel">
    <div className="keys-heading-row"><PageHeading eyebrow={copy.keysEyebrow} title={copy.keysTitle} subtitle={copy.keysSubtitle} /><button ref={createTriggerRef} className="btn btn-primary keys-create-button" type="button" onClick={() => { setCreateOpen(true); setError(null); }}>＋ {localCopy.createKey}</button></div>
    <div className="keys-health-grid" aria-label={localCopy.keyHealthSummary}>
      <article className="keys-health-card keys-health-good"><span>{localCopy.usableNow}</span><strong>{usableCount}</strong><small>{localCopy.usableNowHelp}</small></article>
      <article className={`keys-health-card${blockedCount > 0 ? " keys-health-danger" : ""}`}><span>{localCopy.blockedNow}</span><strong>{blockedCount}</strong><small>{localCopy.blockedNowHelp}</small></article>
      <article className={`keys-health-card${watchlistCount > 0 ? " keys-health-warn" : ""}`}><span>{localCopy.watchlist}</span><strong>{watchlistCount}</strong><small>{localCopy.watchlistHelp}</small></article>
      <article className="keys-health-card keys-health-spend"><span>{localCopy.totalKeySpend}</span><strong>{formatNanoUsd(totalKeySpend)}</strong><small>{localCopy.totalKeySpendHelp}</small></article>
    </div>
    <QuickConnectDock key={issued ? "with-key" : "without-key"} issuedKey={issued} defaultExpanded={keys.length === 0} onDismissKey={() => setIssued(null)} />
    {error && !createOpen && !editTarget && !revokeTarget && <div className="banner banner-error" role="alert">{error}</div>}

    <section className="dsec keys-manager" aria-label={copy.keysTitle}>
      <div className="keys-manager-head"><div><span className="eyebrow">{copy.keysEyebrow}</span><h2>{localCopy.keysListTitle}</h2></div><span>{interpolate(localCopy.keysListSummary, { shown: sortedKeys.length, total: keys.length })}</span></div>
      <div className="keys-toolbar">
        <label className="keys-search"><span aria-hidden="true">⌕</span><input value={search} onChange={(event) => setSearch(event.target.value)} placeholder={localCopy.searchKeys} aria-label={localCopy.searchKeys} /></label>
        <div className="keys-toolbar-right">
          <label className="keys-sort"><span>{localCopy.sortBy}</span><select value={sort} onChange={(event) => setSort(event.target.value as typeof sort)}><option value="newest">{localCopy.sortNewest}</option><option value="name">{localCopy.sortName}</option><option value="spend">{localCopy.sortSpend}</option><option value="last-used">{localCopy.sortLastUsed}</option></select></label>
          <div className="keys-filter-tabs" role="group" aria-label={localCopy.filterLabel}>
            {(["current", "working", "attention", "disabled", "all"] as const).map((status) => <button key={status} type="button" data-key-filter={status} className={`keys-filter-tab ${filter === status ? "on" : ""}`} aria-pressed={filter === status} onClick={() => setFilter(status)}><span>{status === "current" ? localCopy.currentFilter : status === "working" ? localCopy.workingFilter : status === "attention" ? localCopy.attentionFilter : status === "disabled" ? localCopy.disabledFilter : localCopy.allFilter}</span><b>{counts[status]}</b></button>)}
          </div>
        </div>
      </div>

      <div className="key-table-wrap"><table className="key-table">
        <thead><tr><th>{localCopy.colName}</th><th>{localCopy.colKey}</th><th>{localCopy.colSpend}</th><th>{localCopy.colExpires}</th><th>{localCopy.colStatus}</th><th><span className="sr-only">{localCopy.colActions}</span></th></tr></thead>
        <tbody>{sortedKeys.length === 0 ? <tr><td colSpan={6} className="empty-cell"><div className="keys-empty"><strong>{emptyMessage}</strong>{keys.length === 0 ? <button type="button" className="btn btn-primary btn-sm" onClick={() => { setCreateOpen(true); setError(null); }}>{localCopy.createFirstKey}</button> : search.trim() ? <button type="button" className="btn btn-ghost btn-sm" onClick={() => setSearch("")}>{localCopy.clearSearch}</button> : filter !== "current" ? <button type="button" className="btn btn-ghost btn-sm" onClick={() => setFilter("current")}>{localCopy.viewCurrentKeys}</button> : null}</div></td></tr> : sortedKeys.map((key) => {
          const policy = keyPolicy(key, policyNow);
          const health = policy.health;
          const statusText = key.status === "disabled"
            ? localCopy.disabledStatus
            : health === "expired" ? localCopy.expiredStatus
              : health === "limit" ? localCopy.limitReachedStatus
                : health === "expires-soon" ? localCopy.expiresSoonStatus
                  : health === "near-limit" ? localCopy.nearLimitStatus
                    : localCopy.activeStatus;
          const committed = BigInt(key.spentNano) + BigInt(key.reservedNano ?? "0");
          const limit = key.spendLimitNano ? BigInt(key.spendLimitNano) : null;
          const usagePercent = limit && limit > 0n ? Number((committed * 10_000n) / limit > 10_000n ? 10_000n : (committed * 10_000n) / limit) / 100 : 0;
          const usageText = limit
            ? interpolate(localCopy.spentOfLimit, { spent: formatNanoUsd(committed), limit: formatNanoUsd(limit) })
            : interpolate(localCopy.spentWithoutLimit, { spent: formatNanoUsd(committed) });
          return <tr key={key.id} className={`key-row key-row-${health}`}>
            <td data-label={localCopy.colName} className="key-name-cell"><strong>{key.label || copy.unlabelledKey}</strong><span>{interpolate(localCopy.createdOn, { date: new Date(key.createdAt).toLocaleDateString(locale) })}</span></td>
            <td data-label={localCopy.colKey} className="key-credential-cell"><code className="key-mask">{key.keyMasked}</code><span>{localCopy.colLastUsed}: {key.lastUsedAt ? formatRelativeDate(key.lastUsedAt, language) : localCopy.neverUsed}</span></td>
            <td data-label={localCopy.colSpend} className="key-usage-cell"><div><strong>{formatNanoUsd(committed)}</strong><span>{limit ? `/ ${formatNanoUsd(limit)}` : localCopy.unlimited}</span></div>{limit && <span className={`key-usage-track${policy.limitReached || policy.nearLimit ? " warn" : ""}`} role="progressbar" aria-label={usageText} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(usagePercent)}><i style={{ width: `${usagePercent}%` }} /></span>}<small>{usageText}</small></td>
            <td data-label={localCopy.colExpires} className="key-guardrail-cell"><span className={policy.expired || policy.expiresSoon ? "key-policy-warn" : ""}><b>{localCopy.colExpires}</b><em>{key.expiresAt ? new Date(key.expiresAt).toLocaleDateString(locale) : localCopy.never}</em>{policy.expiresSoon && !policy.expired && <small>{localCopy.expiresSoonStatus}</small>}</span></td>
            <td data-label={localCopy.colStatus}><span className={`key-status key-status-${health}`}><i aria-hidden="true" />{statusText}</span></td>
            <td data-label={localCopy.colActions} className="key-actions-cell"><div className="key-actions"><button type="button" className="key-edit-action" data-key-action="edit" disabled={busy} onClick={(event) => openEdit(key, event.currentTarget)}>{localCopy.editKey}</button><details className="key-menu"><summary aria-label={`${localCopy.moreActions}: ${key.label || copy.unlabelledKey}`}>•••</summary><div className="key-menu-pop"><Link href={DOCS_URL} target="_blank" rel="noreferrer">{localCopy.openDocs} ↗</Link>{key.status === "active" && <button type="button" className="danger" disabled={busy} onClick={(event) => { const details = event.currentTarget.closest("details"); const summary = details?.querySelector<HTMLElement>("summary"); details?.removeAttribute("open"); openRevoke(key, summary); }}>{localCopy.revokeKey}</button>}</div></details></div></td>
          </tr>;
        })}</tbody>
      </table></div>

    </section>

    {createOpen && <div className="modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.currentTarget === event.target) closeCreate(); }}><form ref={createModalRef} className="key-modal" role="dialog" aria-modal="true" aria-labelledby="create-key-title" aria-describedby="create-key-description" tabIndex={-1} onSubmit={create}>
      <div className="key-modal-head"><div><span className="eyebrow">{copy.keysEyebrow}</span><h2 id="create-key-title">{localCopy.createKeyTitle}</h2><p id="create-key-description">{localCopy.createKeyHelp}</p></div><button type="button" className="key-modal-close" onClick={closeCreate} aria-label={localCopy.cancel}>×</button></div>
      <div className="key-modal-fields">
        <label className="key-field key-field-wide"><span>{localCopy.keyName} <small>{localCopy.optional}</small></span><input className="set-in" value={label} onChange={(event) => { setLabel(event.target.value); setError(null); }} maxLength={64} placeholder={localCopy.keyNameHint} autoFocus /><em>{localCopy.keyNameHelp}</em></label>
        <fieldset className="key-create-guardrails"><legend>{localCopy.guardrailsTitle} <small>{localCopy.optional}</small></legend><p>{localCopy.guardrailsHelp}</p><div className="key-create-guardrail-grid">
          <label className="key-field"><span>{localCopy.spendLimit}</span><div className="key-money-field"><b>$</b><input className="set-in" inputMode="decimal" value={spendLimit} onChange={(event) => { setSpendLimit(event.target.value); setError(null); }} placeholder="100.00" /></div><em>{localCopy.spendLimitHint}</em></label>
          <label className="key-field"><span>{localCopy.expiration}</span><input className="set-in" type="date" min={today} value={expirationDate} onChange={(event) => { setExpirationDate(event.target.value); setError(null); }} /><em>{expirationDate ? localCopy.expirationHint : localCopy.noExpiration}</em></label>
        </div></fieldset>
        {user.totpEnabled && <label className="key-field key-field-wide"><span>{copy.twoFactorCodeLabel}</span><input className="set-in tfa-code" inputMode="numeric" autoComplete="one-time-code" maxLength={6} value={totpCode} onChange={(event) => { setTotpCode(event.target.value.replace(/\D/g, "").slice(0, 6)); setError(null); }} placeholder={copy.twoFactorCodePlaceholder} /></label>}
      </div>
      {error && <div className="banner banner-error" role="alert">{error}</div>}
      <div className="key-modal-actions"><button type="button" className="btn btn-ghost" disabled={busy} onClick={closeCreate}>{localCopy.cancel}</button><button className="btn btn-primary" disabled={busy}>{busy ? localCopy.creating : localCopy.createKey}</button></div>
    </form></div>}

    {editTarget && <div className="modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.currentTarget === event.target) closeEdit(); }}><form ref={editModalRef} className="key-modal key-edit-modal" role="dialog" aria-modal="true" aria-labelledby="edit-key-title" aria-describedby="edit-key-description" tabIndex={-1} onSubmit={updateKey}>
      <div className="key-modal-head"><div><span className="eyebrow">{localCopy.editKey}</span><h2 id="edit-key-title">{localCopy.editKeyTitle}</h2><p id="edit-key-description">{localCopy.editKeyHelp}</p></div><button type="button" className="key-modal-close" disabled={busy} onClick={closeEdit} aria-label={localCopy.cancel}>×</button></div>
      <div className="key-policy-summary"><div><span>{localCopy.colKey}</span><code>{editTarget.keyMasked}</code></div><div><span>{localCopy.committedSpend}</span><b>{formatNanoUsd(policyCommittedNano)}</b></div></div>
      {(editTargetState?.expired || editTargetState?.limitReached) && <p className="key-policy-reactivate"><span aria-hidden="true">ⓘ</span>{localCopy.policyReactivates}</p>}
      <div className="key-modal-fields">
        <label className="key-field key-field-wide"><span>{localCopy.keyName}</span><input className="set-in" value={editLabel} onChange={(event) => { setEditLabel(event.target.value); setError(null); }} maxLength={64} placeholder={localCopy.keyNameHint} autoFocus /></label>
        {editTarget.status === "active" && <><label className="key-field"><span>{localCopy.spendLimit}</span><div className="key-money-field"><b>$</b><input className="set-in" inputMode="decimal" value={policySpendLimit} onChange={(event) => { setPolicySpendLimit(event.target.value); setError(null); }} placeholder={localCopy.unlimited} /></div><em>{localCopy.policyLimitHint}</em></label>
        <label className="key-field"><span>{localCopy.expiration}</span><input className="set-in" type="date" min={today} value={policyExpirationDate} onChange={(event) => { setPolicyExpirationDate(event.target.value); setError(null); }} /><em>{localCopy.policyExpirationHint}</em></label>
        {policyDirty && user.totpEnabled && <label className="key-field key-field-wide"><span>{copy.twoFactorCodeLabel}</span><input className="set-in tfa-code" inputMode="numeric" autoComplete="one-time-code" maxLength={6} value={policyTotpCode} onChange={(event) => { setPolicyTotpCode(event.target.value.replace(/\D/g, "").slice(0, 6)); setError(null); }} placeholder={copy.twoFactorCodePlaceholder} /></label>}</>}
      </div>
      {error && <div className="banner banner-error" role="alert">{error}</div>}
      <div className="key-modal-actions"><button type="button" className="btn btn-ghost" disabled={busy} onClick={closeEdit}>{localCopy.cancel}</button><button className="btn btn-primary" disabled={busy || !editDirty}>{busy ? localCopy.savingPolicy : localCopy.savePolicy}</button></div>
    </form></div>}

    {revokeTarget && <div className="modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.currentTarget === event.target) closeRevoke(); }}><div ref={revokeModalRef} className="key-modal key-revoke-modal" role="alertdialog" aria-modal="true" aria-labelledby="revoke-key-title" aria-describedby="revoke-key-description" tabIndex={-1}><div className="key-modal-head"><div><span className="eyebrow danger-text">{localCopy.revokeKey}</span><h2 id="revoke-key-title">{localCopy.revokeTitle}</h2><p><strong>{revokeTarget.label || copy.unlabelledKey}</strong> · <code>{revokeTarget.keyMasked}</code></p></div></div><p id="revoke-key-description">{localCopy.revokeBody}</p>{error && <div className="banner banner-error" role="alert">{error}</div>}<div className="key-modal-actions"><button type="button" className="btn btn-ghost" disabled={busy} onClick={closeRevoke}>{localCopy.cancel}</button><button type="button" className="btn btn-danger" disabled={busy} autoFocus onClick={() => void revoke()}>{localCopy.confirmRevoke}</button></div></div></div>}
  </section>;
}

function TerminalCommands({ commands }: { commands: string }) {
  return <code>{commands.split("\n").map((command, index) => {
    const assignmentEnd = command.indexOf("=") + 1;
    const prefix = assignmentEnd > 0 ? command.slice(0, assignmentEnd) : command;
    const value = assignmentEnd > 0 ? command.slice(assignmentEnd) : "";
    return <span className="agent-terminal-line" key={`${index}-${command}`}>{prefix}{assignmentEnd > 0 && <wbr />}{value || "\u00a0"}</span>;
  })}</code>;
}

function QuickConnectDock({ issuedKey, defaultExpanded, onDismissKey }: { issuedKey: string | null; defaultExpanded: boolean; onDismissKey(): void }) {
  const copy = useDashboardCopy();
  const { language } = useI18n();
  const [expanded, setExpanded] = useState(Boolean(issuedKey) || defaultExpanded);
  const handoff = buildClaudeAgentHandoff({ apiKey: issuedKey, docsUrl: DOCS_URL, language });
  const terminalCommands = buildClaudeCodeCommands(issuedKey);

  return <aside className={`agent-connect-dock${expanded ? " is-open" : ""}${issuedKey ? " has-live-key" : ""}`} aria-labelledby="agent-connect-title">
    <button className="agent-connect-summary" type="button" aria-expanded={expanded} aria-controls="agent-connect-body" onClick={() => setExpanded((current) => !current)}>
      <span className="agent-connect-icon" aria-hidden="true">&gt;_</span>
      <span className="agent-connect-main"><span>{copy.agentDockEyebrow}</span><strong id="agent-connect-title">{copy.agentDockTitle}</strong><small>{copy.agentDockText}</small></span>
      <span className={`agent-connect-state${issuedKey ? " ready" : ""}`}><i />{issuedKey ? copy.agentDockKeyIncluded : copy.agentDockKeyPlaceholder}</span>
      <span className="agent-connect-chevron" aria-hidden="true">⌄</span>
    </button>
    {expanded && <div className="agent-connect-body" id="agent-connect-body">
      {issuedKey && <div className="agent-key-reveal secret-card"><div className="agent-key-reveal-head"><div><strong>{copy.copyNewKeyNow}</strong><span>{copy.rawSecretWarning}</span></div><span className="chip">{copy.shownOnce}</span></div><div className="secret-key-field"><code>{issuedKey}</code><CopyButton value={issuedKey} className="secret-copy" /></div></div>}
      <div className="agent-connect-path" aria-label={copy.agentDockEyebrow}><span><b>1</b>{copy.agentDockStepOne}</span><i>→</i><span><b>2</b>{copy.agentDockStepTwo}</span><i>→</i><span><b>3</b>{copy.agentDockStepThree}</span></div>
      <div className="agent-terminal" aria-label={copy.agentDockTerminal}>
        <div className="agent-terminal-head"><span><i /><i /><i />{copy.agentDockTerminal}</span><CopyButton value={terminalCommands} className="agent-connect-copy" label={issuedKey ? copy.agentDockCopyTerminal : copy.agentDockCopyTemplate} copiedLabel={copy.agentDockTerminalCopied} /></div>
        <pre><TerminalCommands commands={terminalCommands} /></pre>
      </div>
      <div className="agent-connect-footer"><div><strong>{copy.agentDockAgentTitle}</strong><span>{copy.agentDockAgentText}</span></div><div className="agent-connect-footer-actions"><CopyButton value={handoff} className="agent-handoff-copy" label={issuedKey ? copy.agentDockCopyWithKey : copy.agentDockCopy} copiedLabel={copy.agentDockCopied} /><Link className="btn btn-ghost btn-sm" href={DOCS_URL} target="_blank" rel="noreferrer">{copy.agentDockDocs} ↗</Link>{issuedKey && <button type="button" className="btn btn-ghost btn-sm" onClick={onDismissKey}>{copy.savedKey}</button>}</div></div>
      <p className={`agent-connect-note${issuedKey ? " secret-note" : ""}`}><span aria-hidden="true">{issuedKey ? "⚠" : "ⓘ"}</span><span>{issuedKey ? copy.agentDockSecretNote : copy.agentDockTemplateNote}</span></p>
    </div>}
  </aside>;
}

function nanoToUsdInput(value: string): string {
  const nano = BigInt(value);
  const whole = nano / NANO_PER_USD;
  const fraction = (nano % NANO_PER_USD).toString().padStart(9, "0").replace(/0+$/, "");
  return fraction ? `${whole}.${fraction}` : whole.toString();
}

function usdInputToNano(value: string): bigint {
  const [whole = "0", fraction = ""] = value.split(".");
  return BigInt(whole) * NANO_PER_USD + BigInt(fraction.padEnd(9, "0"));
}

function isoToLocalDateInput(value: string): string {
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) return "";
  return new Date(date.getTime() - date.getTimezoneOffset() * 60_000).toISOString().slice(0, 10);
}

function keyPolicy(key: ApiKeyView, now: number): {
  health: "active" | "disabled" | "expired" | "expires-soon" | "limit" | "near-limit";
  expired: boolean; expiresSoon: boolean; limitReached: boolean; nearLimit: boolean;
} {
  const expired = Boolean(key.expiresAt && Date.parse(key.expiresAt) <= now);
  const expiresSoon = Boolean(key.expiresAt && !expired && Date.parse(key.expiresAt) - now <= 7 * 86_400_000);
  let limitReached = false, nearLimit = false;
  if (key.spendLimitNano) {
    const committed = BigInt(key.spentNano) + BigInt(key.reservedNano ?? "0");
    const limit = BigInt(key.spendLimitNano);
    limitReached = committed >= limit;
    nearLimit = !limitReached && committed * 10n >= limit * 9n;
  }
  const health = key.status === "disabled" ? "disabled" : expired ? "expired" : limitReached ? "limit" : nearLimit ? "near-limit" : expiresSoon ? "expires-soon" : "active";
  return { health, expired, expiresSoon, limitReached, nearLimit };
}

export function isApiKeyUsable(key: ApiKeyView, now: number): boolean {
  const policy = keyPolicy(key, now);
  return key.status === "active" && !policy.expired && !policy.limitReached;
}

function formatRelativeDate(value: string, language: "en" | "ru"): string {
  const elapsedDays = Math.floor((Date.now() - Date.parse(value)) / 86_400_000);
  if (elapsedDays <= 0) return language === "ru" ? "Сегодня" : "Today";
  if (elapsedDays === 1) return language === "ru" ? "Вчера" : "Yesterday";
  if (elapsedDays < 30) return language === "ru" ? `${elapsedDays} дн. назад` : `${elapsedDays}d ago`;
  return new Date(value).toLocaleDateString(language === "ru" ? "ru-RU" : "en-US");
}

const TOPUP_PRESETS = [100, 250, 500, 1000] as const;

function Credits({ account, ledger, ledgerAvailable }: { account: AccountView; ledger: LedgerEntry[]; ledgerAvailable: boolean }) {
  const copy = useDashboardCopy();
  const { language } = useI18n();
  const localCopy = localDashboardCopy[language];
  const [amount, setAmount] = useState("100");
  const [method, setMethod] = useState<number>(PLATEGA_METHODS[0]!.id);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [checkout, setCheckout] = useState<CheckoutView | null>(null);
  const amountValid = /^[1-9]\d*$/.test(amount);
  const amountValidation = amount === "" || amountValid ? null : localCopy.invalidWholeUsd;
  async function start() {
    if (!amountValid) { setError(localCopy.invalidWholeUsd); return; }
    setBusy(true); setError(null);
    try {
      const created = await api.createCheckout(amount, method); setCheckout(created);
      const methodName = method === 2 ? "sbp" : method === 11 ? "card" : method === 13 ? "crypto" : "other";
      trackProductEvent("Checkout Created", { provider: created.provider, payment_method: methodName, amount_bucket: checkoutAmountBucket(amount) });
      trackFirstProductEvent("checkout", "First Checkout Created", { provider: created.provider, payment_method: methodName, amount_bucket: checkoutAmountBucket(amount) });
      if (created.checkoutUrl) {
        const checkoutUrl = safeCheckoutUrl(created.checkoutUrl, created.provider);
        if (!checkoutUrl) { setError(localCopy.invalidCheckoutUrl); return; }
        window.location.assign(checkoutUrl);
      }
    } catch (cause) { setError(cause instanceof Error ? cause.message : copy.createCheckoutError); }
    finally { setBusy(false); }
  }

  // Prepay-модель: тир определяется НАКОПЛЕННОЙ суммой пополнений (не расходом). Показываем, какой
  // тир даёт пополнение на введённую сумму, ценность по его скидке и условие удержания (50%/30 дней).
  const pricing = account.pricing;
  // Партнёрская фикс-ставка → выходим из прогрессивной тир-логики (ставка не зависит от суммы пополнения).
  const partnerRate = isPartnerRate(account);
  const isB2c = pricing?.customerType === "b2c" && !partnerRate;
  const fixedRateName = partnerRate ? copy.partnerRate : copy.businessRate;
  const amountNano = amountValid ? BigInt(amount) * NANO_PER_USD : 0n;
  const currentIdx = pricing?.customerType === "b2c" ? B2C_PRICING_MILESTONES.findIndex((milestone) => milestone.code === pricing.tier) : -1;
  const cumulativeNano = pricing?.customerType === "b2c" ? BigInt(pricing.spentNano) + amountNano : amountNano;
  const projectedIdx = isB2c ? tierIndexForCumulativeNano(cumulativeNano) : -1;
  const reachedIdx = isB2c ? Math.max(currentIdx, projectedIdx) : -1;
  const reachedTier = reachedIdx >= 0 ? B2C_PRICING_MILESTONES[reachedIdx] : null;
  const discount = isB2c ? (reachedTier?.discountPercent ?? 0) : discountOf(account);
  const topupPaymentBp = isB2c ? BigInt(100 - discount) * 100n : paymentBasisPoints(account);
  const hasTier = discount > 0;
  const apiValueNano = officialNanoFromCharged(amountNano, topupPaymentBp);
  const nextTier = isB2c && reachedIdx + 1 < B2C_PRICING_MILESTONES.length ? B2C_PRICING_MILESTONES[reachedIdx + 1] : null;
  const nextTierRemainingNano = nextTier ? bigintMax(0n, BigInt(nextTier.spendThresholdNano) - cumulativeNano) : 0n;
  const balanceApiNano = officialNanoFromCharged(BigInt(account.balanceNano), paymentBasisPoints(account));
  const apiValueForPreset = (preset: number) => {
    const presetNano = BigInt(preset) * NANO_PER_USD;
    if (!isB2c || pricing?.customerType !== "b2c") return officialNanoFromCharged(presetNano, paymentBasisPoints(account));
    const index = Math.max(currentIdx, tierIndexForCumulativeNano(BigInt(pricing.spentNano) + presetNano));
    const tier = B2C_PRICING_MILESTONES[Math.max(0, index)]!;
    return officialNanoFromCharged(presetNano, BigInt(100 - tier.discountPercent) * 100n);
  };
  const topups = ledger.filter((entry) => entry.kind === "topup");
  const ledgerMayBePartial = ledger.length >= 100;

  return <section className="panel"><PageHeading eyebrow={copy.creditsEyebrow} title={copy.creditsTitle} subtitle={copy.creditsSubtitle} />
    <div className="credits-stack">
      <div className="ov-stats bill3 tc-stats">
        <div className="ovstat"><span className="dlabel">{copy.currentBalance}</span><b className="num">{normalizeUsd(account.balanceUsd)}</b><span className="dtrend">{BigInt(account.balanceNano) > 0n ? interpolate(copy.valueOfBalance, { value: formatNanoUsd(balanceApiNano) }) : copy.available}</span></div>
        <Stat label={copy.used} value={formatNanoUsd(account.spentNano)} detail={copy.balanceAfterDiscount} />
        <div className="ovstat"><span className="dlabel">{partnerRate ? copy.partnerRateLabel : copy.currentTier}</span><b className="num tc-tier-name">{isB2c ? (currentIdx >= 0 ? tierName(copy, B2C_PRICING_MILESTONES[currentIdx].code) : copy.noTierYet) : fixedRateName}</b><span className="dtrend">{discountOf(account)}% {copy.discount} · {formatMultiplier(paymentBasisPoints(account))} {copy.valueMultiplier}</span></div>
      </div>

      <div className="card topup-convert">
        <div className="tc-head"><h2>{copy.anyWholeAmount}</h2><p className="p-sub" id="topup-amount-help">{copy.checkoutHelp}</p></div>
        <div className="tc-body">
          <div className="tc-input">
            <label className="tc-field"><span className="currency-prefix">$</span><input className="set-in" inputMode="numeric" pattern="[1-9][0-9]*" value={amount} onChange={(event) => { setAmount(event.target.value); setError(null); }} placeholder="100" aria-label={copy.anyWholeAmount} aria-describedby={amountValidation ? "topup-amount-help topup-amount-error" : "topup-amount-help"} aria-invalid={amountValidation ? true : undefined} /></label>
            <div className="tc-presets" role="group" aria-label={copy.quickAmounts}>{TOPUP_PRESETS.map((preset) => <button key={preset} type="button" className={`tc-preset ${amount === String(preset) ? "on" : ""}`} data-topup-preset={preset} aria-pressed={amount === String(preset)} onClick={() => { setAmount(String(preset)); setError(null); }}><b>${preset}</b><span>{formatNanoUsd(apiValueForPreset(preset))}</span></button>)}</div>
          </div>
          <div className="tc-arrow" aria-hidden="true">→</div>
          <div className={`tc-receive ${hasTier ? "tc-receive-up" : ""}`}>
            <span className="tc-recv-label">{copy.youReceive}</span>
            <b className="tc-recv-value">{amountNano > 0n ? `≈ ${formatNanoUsd(apiValueNano)}` : "—"}</b>
            <span className="tc-recv-sub">{amountNano <= 0n ? copy.enterAmount : hasTier ? `${copy.inClaudeApi} · ${interpolate(copy.atTier, { tier: reachedTier ? tierName(copy, reachedTier.code) : fixedRateName, discount })}` : `${copy.inClaudeApi} · ${copy.noDiscountYet}`}</span>
            <div className="tc-recv-meta"><span className="tc-badge">−{discount}%</span><span className="tc-badge tc-badge-soft">{formatMultiplier(topupPaymentBp)} {copy.valueMultiplier}</span></div>
          </div>
        </div>
        {isB2c && amountNano > 0n && reachedTier && BigInt(reachedTier.holdUsd) > 0n &&
          <p className="tc-upgrade"><span className="tc-upgrade-ic" aria-hidden="true">ⓘ</span>{interpolate(copy.topupReach, { tier: tierName(copy, reachedTier.code), discount, hold: formatWholeUsd(reachedTier.holdUsd) })}</p>}
        <p className="tc-explain">{hasTier ? interpolate(copy.perDollar, { mult: formatPerDollar(topupPaymentBp) }) : copy.perDollarNone}</p>
        {isB2c && amountNano > 0n && nextTier && nextTierRemainingNano > 0n && <p className="tc-nudge">↑ {interpolate(localCopy.topupNextRemaining, { amount: formatNanoUsd(nextTierRemainingNano), tier: tierName(copy, nextTier.code), discount: nextTier.discountPercent })}</p>}
        <div className="tc-pay">
          <span className="tc-pay-label">{localCopy.payWith}</span>
          <div className="tc-methods" role="radiogroup" aria-label={localCopy.payWith}>
            {PLATEGA_METHODS.map((m) => <button key={m.id} type="button" role="radio" className={`pm-card ${method === m.id ? "on" : ""}`} aria-checked={method === m.id} onClick={() => setMethod(m.id)}>
              <span className={`pm-ic${"logo" in m ? " pm-ic-logo" : ""}`} aria-hidden="true">{m.icon}</span>
              <span className="pm-txt"><b>{language === "ru" ? m.ru : m.en}</b><span>{language === "ru" ? m.ruDesc : m.enDesc}</span></span>
            </button>)}
          </div>
        </div>
        <div className="tc-actions"><button className="btn btn-primary" disabled={busy || !amountValid} onClick={start}>{busy ? copy.creating : copy.continuePayment}</button></div>
        {amountValidation && <div className="auth-msg err" id="topup-amount-error">{amountValidation}</div>}
        {error && <div className="auth-msg err">{error}</div>}{checkout && !checkout.checkoutUrl && <div className="banner">{interpolate(copy.checkoutPending, { id: checkout.id, status: checkout.status })}</div>}
      </div>

      <PricingBanner account={account} />

      {ledgerAvailable && ledgerMayBePartial && <div className="banner">{localCopy.partialLedger}</div>}
      {ledgerAvailable && <section className="dsec credits-history"><div className="dsec-head"><h2 id="topup-history-title">{copy.topupHistory}</h2></div>
        <div className="table-scroll"><table className="mtable topup-history-table" aria-labelledby="topup-history-title" role="table">
          <thead role="rowgroup"><tr role="row"><th scope="col" role="columnheader">{copy.date}</th><th scope="col" role="columnheader" className="tnum">{copy.histPaid}</th><th scope="col" role="columnheader">{copy.histDiscount}</th><th scope="col" role="columnheader" className="tnum">{copy.histApiValue}</th><th scope="col" role="columnheader">{copy.reference}</th></tr></thead>
          <tbody role="rowgroup">{topups.length === 0 ? <tr role="row"><td role="cell" colSpan={5} className="empty-cell">{copy.noTopups}</td></tr> : topups.map((entry) => {
            const d = (entry as { discountPercent?: number }).discountPercent ?? discountOf(account);
            const paidNano = BigInt(entry.amountNano);
            const officialValueNano = officialNanoFromCharged(paidNano, BigInt(100 - d) * 100n);
            return <tr role="row" key={entry.id}>
              <td role="cell" data-label={copy.date}>{formatLedgerTime(entry.timestamp, language)}</td>
              <td role="cell" className="tnum" data-label={copy.histPaid}>{formatNanoUsd(paidNano)}</td>
              <td role="cell" data-label={copy.histDiscount}><span className="pill pill-soft">−{d}%</span></td>
              <td role="cell" className="tnum" data-label={copy.histApiValue}>≈ {formatNanoUsd(officialValueNano)}</td>
              <td role="cell" data-label={copy.reference}>{entry.reference ?? "—"}</td>
            </tr>;
          })}</tbody>
        </table></div>
      </section>}
    </div>
  </section>;
}


function Usage({ account, ledger, usage, ledgerAvailable }: { account: AccountView; ledger: LedgerEntry[]; usage: UsageView; ledgerAvailable: boolean }) {
  const copy = useDashboardCopy();
  const { language } = useI18n();
  const localCopy = localDashboardCopy[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const models = usage.models;
  const modelOfficialTotal = models.reduce((sum, model) => sum + BigInt(model.officialNano), 0n);

  // Скидка определяет, сколько реального Claude API стоит каждый списанный доллар:
  // клиент платит multiplierBp от официальной цены → официальная ценность = списано × 10000 / multiplierBp.
  const multiplierBp = paymentBasisPoints(account);
  // discountOf учитывает партнёрский пол (effectiveDiscountPercent), тогда как pricing.discountPercent —
  // это тир-скидка. Для реферала с фикс-ставкой показываем реальную (эффективную) скидку.
  const discount = discountOf(account);
  const netChargedNano = BigInt(account.spentNano);
  const officialReceivedNano = officialNanoFromCharged(netChargedNano, multiplierBp);

  const charges = ledger.filter((entry) => entry.kind === "charge");
  const ledgerMayBePartial = ledger.length >= 100;

  // Стабильный цвет на модель: сначала порядок из агрегата usage.models (совпадает с таблицей ниже),
  // затем модели, встреченные только в ledger. Один и тот же id → один цвет во всех графиках.
  const modelColor = new Map<string, string>();
  const assignColor = (id: string) => { if (!modelColor.has(id)) modelColor.set(id, MODEL_COLORS[modelColor.size % MODEL_COLORS.length]!); };
  for (const model of models) assignColor(model.model);

  // Match the authoritative /usage?window=30d aggregate: today plus the preceding 29 local days.
  // The bars still come from the bounded ledger endpoint, so they are explicitly labelled as visible entries.
  const [nowMs] = useState(() => Date.now());
  const todayMs = startOfDay(nowMs);
  const today = new Date(todayMs);
  const days = Array.from({ length: 30 }, (_, index) => new Date(today.getFullYear(), today.getMonth(), today.getDate() - (29 - index)).getTime());

  // День → (модель → официальная ценность $). Модель берём из charge.model, иначе «прочее».
  const UNKNOWN_MODEL = "__other__";
  const perDay = new Map<number, Map<string, bigint>>();
  for (const charge of charges) {
    const bucket = startOfDay(ledgerMs(charge.timestamp));
    if (bucket < days[0]! || bucket > days[days.length - 1]!) continue;
    const id = charge.model || UNKNOWN_MODEL;
    assignColor(id);
    const slot = perDay.get(bucket) ?? new Map<string, bigint>();
    const officialNano = officialNanoFromCharged(BigInt(charge.amountNano), multiplierBp);
    slot.set(id, (slot.get(id) ?? 0n) + officialNano);
    perDay.set(bucket, slot);
  }
  const series = days.map((day) => {
    const byModel = perDay.get(day);
    const segs = byModel ? [...byModel.entries()].map(([id, value]) => ({ id, value })).sort((a, b) => compareBigInt(b.value, a.value)) : [];
    return { day, value: segs.reduce((sum, seg) => sum + seg.value, 0n), segs };
  });
  const maxValue = series.reduce((max, point) => bigintMax(max, point.value), 0n);
  const scale = niceNanoScale(maxValue);
  const gridTicks = Array.from({ length: scale.divisions + 1 }, (_, index) => scale.max - BigInt(index) * scale.step); // сверху вниз
  const summaryOfficialNano = BigInt(usage.totalOfficialNano);
  const summaryRequests = usage.requests;
  const peak = series.reduce((best, point) => (point.value > best.value ? point : best), { day: todayMs, value: 0n, segs: [] as { id: string; value: bigint }[] });
  const LABEL_COUNT = 7;
  const axisMarks = [...new Set(Array.from({ length: LABEL_COUNT }, (_, i) => Math.round(i * (days.length - 1) / (LABEL_COUNT - 1))))];

  // Разбивка модель-бара (mdist) с центрами сегментов — для наведения/подсказки.
  const modelShares = models.map((model) => modelOfficialTotal > 0n ? boundedRatio(BigInt(model.officialNano), modelOfficialTotal) : 1 / models.length);
  const mdistPlaced = models.map((model, index) => {
    const share = modelShares[index]!;
    const center = modelShares.slice(0, index).reduce((sum, value) => sum + value, 0) + share / 2;
    return { model, share, center };
  });
  const [hoverDay, setHoverDay] = useState<number | null>(null);
  const [mdistHover, setMdistHover] = useState<number | null>(null);

  // Разбивка списаний по API-ключу — наш аналог per-endpoint из референса.
  const keyMap = new Map<string, { key: string; count: number; netNano: bigint }>();
  for (const charge of charges) {
    const key = charge.keyMasked ?? "__system__";
    const row = keyMap.get(key) ?? { key, count: 0, netNano: 0n };
    row.count += 1;
    row.netNano += BigInt(charge.amountNano);
    keyMap.set(key, row);
  }
  const keyRows = [...keyMap.values()].sort((a, b) => compareBigInt(b.netNano, a.netNano));
  const sampledChargedNano = charges.reduce((sum, charge) => sum + BigInt(charge.amountNano), 0n);
  const sampledOfficialNano = officialNanoFromCharged(sampledChargedNano, multiplierBp);

  return <section className="panel"><PageHeading eyebrow={copy.usageEyebrow} title={copy.usageTitle} subtitle={copy.usageSubtitle} />
    <div className="banner">💡 <b>{copy.sessionSavingTitle}</b><span> {copy.sessionSavingText}</span></div>
    {ledgerAvailable && ledgerMayBePartial && <div className="banner">{localCopy.partialLedger}</div>}

    <div className="ov-stats bill4">
      <div className="ovstat"><span className="dlabel">{copy.claudeApiReceived}</span><b className="num accent">{formatNanoUsd(officialReceivedNano)}</b><span className="dtrend">{copy.atOfficialPrices}</span></div>
      <Stat label={copy.balanceCharged} value={formatNanoUsd(account.spentNano)} detail={copy.afterDiscount} />
      <div className="ovstat"><span className="dlabel">{copy.activeDiscount}</span><b className="num">{discount}%</b><span className="dtrend">{formatMultiplier(multiplierBp)} {copy.valueMultiplier}</span></div>
      <div className="ovstat"><span className="dlabel">{copy.availableBalance}</span><b className="num">{normalizeUsd(account.balanceUsd)}</b><span className="dtrend">{BigInt(account.balanceNano) > 0n ? interpolate(copy.valueOfBalance, { value: formatNanoUsd(officialNanoFromCharged(BigInt(account.balanceNano), multiplierBp)) }) : copy.available}</span></div>
    </div>

    <div className={`usage-graph${ledgerAvailable ? "" : " usage-graph-summary-only"}`}>
      {ledgerAvailable && <div className="uchart">
        <div className="uchart-head"><b>{copy.usageOverTime}</b><span>{copy.chartWindowLabel}</span></div>
        {maxValue === 0n ? <div className="uchart-empty">{copy.noChargesPeriod}</div> : <>
          <div className="uchart-grid">
            <div className="uchart-yaxis">{gridTicks.map((tick, i) => <span key={i}>{formatAxisNanoUsd(tick)}</span>)}</div>
            <div className="uchart-plotwrap">
              <div className="uchart-lines">{gridTicks.map((_, i) => <i key={i} />)}</div>
              <div className="uchart-plot" onMouseLeave={(event) => { if (!event.currentTarget.contains(document.activeElement)) setHoverDay(null); }}>
                {series.map((point, index) => <button type="button" key={point.day} className={`uchart-col${hoverDay === index ? " is-hover" : ""}`} aria-label={interpolate(copy.chartDayLabel, { date: fmtDay(point.day, locale), value: formatNanoUsdSmart(point.value) })} onMouseEnter={() => setHoverDay(index)} onFocus={() => setHoverDay(index)} onBlur={() => setHoverDay((current) => current === index ? null : current)} onClick={() => setHoverDay((current) => current === index ? null : index)} onKeyDown={(event) => { if (event.key === "Escape") { setHoverDay(null); event.currentTarget.blur(); } }}>
                  <div className="uchart-col-fill">
                    {point.segs.map((seg) => <div key={seg.id} className="uchart-seg" style={{ height: `${boundedPercent(seg.value, scale.max)}%`, background: modelColor.get(seg.id) }} />)}
                  </div>
                </button>)}
                {hoverDay !== null && series[hoverDay] && series[hoverDay]!.value > 0n && (() => {
                  const point = series[hoverDay]!;
                  const leftPct = Math.min(92, Math.max(8, (hoverDay + 0.5) / days.length * 100));
                  return <div className="chart-tip" role="tooltip" style={{ left: `${leftPct}%`, bottom: `${boundedPercent(point.value, scale.max)}%` }}>
                    <div className="chart-tip-h">{fmtDay(point.day, locale)}</div>
                    {point.segs.map((seg) => <div key={seg.id} className="chart-tip-row"><span className="chart-tip-dot" style={{ background: modelColor.get(seg.id) }} /><span className="chart-tip-nm">{seg.id === UNKNOWN_MODEL ? copy.otherModels : modelLabel(seg.id)}</span><b>{formatNanoUsdSmart(seg.value)}</b></div>)}
                    <div className="chart-tip-total"><span>{copy.chartTotal}</span><b>{formatNanoUsdSmart(point.value)}</b></div>
                  </div>;
                })()}
              </div>
              <div className="uchart-axis">{axisMarks.map((mark) => <span key={mark} style={{ left: `${(mark + 0.5) / days.length * 100}%` }}>{fmtDay(days[mark]!, locale)}</span>)}</div>
            </div>
          </div>
        </>}
      </div>}
      <div className="usum">
        <span className="usum-t">{copy.periodSummary}</span>
        <div className="usum-row"><span>{copy.officialSpend}</span><b className="accent">{formatNanoUsd(summaryOfficialNano)}</b></div>
        <div className="usum-row"><span>{copy.chargeEvents}</span><b>{summaryRequests.toLocaleString(locale)}</b></div>
        {ledgerAvailable && <div className="usum-row"><span>{copy.peakDay}</span><b>{peak.value > 0n ? `${fmtDay(peak.day, locale)} · ${formatNanoUsd(peak.value)}` : "—"}</b></div>}
        <div className="usum-row"><span>{copy.dailyAverage}</span><b>{summaryOfficialNano > 0n ? formatNanoUsd(roundDivide(summaryOfficialNano, 30n)) : "—"}</b></div>
      </div>
    </div>

    <section className="dsec">
      <div className="dsec-head analytics-heading"><div><h2>{copy.tokensAndModels}</h2><p>{copy.tokensAndModelsSub}</p></div></div>
      {models.length === 0 ? <div className="empty-box">{copy.tokensPending}</div> : <>
        <div className="tok-buckets">
          <div className="tokb"><span className="dlabel">{copy.inputTokens}</span><b>{fmtTokens(usage.buckets.input.tokens)}</b><span className="tokb-usd">{fmtNanoUsd(usage.buckets.input.officialNano)}</span></div>
          <div className="tokb"><span className="dlabel">{copy.outputTokens}</span><b>{fmtTokens(usage.buckets.output.tokens)}</b><span className="tokb-usd">{fmtNanoUsd(usage.buckets.output.officialNano)}</span></div>
          <div className="tokb"><span className="dlabel">{copy.cacheReadLabel}</span><b>{fmtTokens(usage.buckets.cacheRead.tokens)}</b><span className="tokb-usd">{fmtNanoUsd(usage.buckets.cacheRead.officialNano)}</span></div>
          <div className="tokb"><span className="dlabel">{copy.cacheWriteLabel}</span><b>{fmtTokens(usage.buckets.cacheWrite.tokens)}</b><span className="tokb-usd">{fmtNanoUsd(usage.buckets.cacheWrite.officialNano)}</span></div>
          {usage.buckets.webSearch.requests > 0 && <div className="tokb"><span className="dlabel">{copy.webSearchLabel}</span><b>{usage.buckets.webSearch.requests.toLocaleString(locale)}</b><span className="tokb-usd">{fmtNanoUsd(usage.buckets.webSearch.officialNano)}</span></div>}
        </div>
        <div className="mdist-wrap">
          <div className="mdist" role="group" aria-label={copy.tokensAndModels} onMouseLeave={(event) => { if (!event.currentTarget.contains(document.activeElement)) setMdistHover(null); }}>
            {mdistPlaced.map((seg, index) => <button type="button" aria-label={`${modelLabel(seg.model.model)} · ${fmtNanoUsd(seg.model.officialNano)} · ${(seg.share * 100).toFixed(seg.share < 0.1 ? 1 : 0)}%`} key={seg.model.model} className={`mdist-seg${mdistHover === index ? " is-hover" : ""}`} style={{ width: `${seg.share * 100}%`, background: modelColor.get(seg.model.model) }} onMouseEnter={() => setMdistHover(index)} onFocus={() => setMdistHover(index)} onBlur={() => setMdistHover((current) => current === index ? null : current)} onClick={() => setMdistHover((current) => current === index ? null : index)} />)}
          </div>
          {mdistHover !== null && mdistPlaced[mdistHover] && (() => {
            const seg = mdistPlaced[mdistHover]!;
            const leftPct = Math.min(92, Math.max(8, seg.center * 100));
            return <div className="chart-tip mdist-tip" role="tooltip" style={{ left: `${leftPct}%` }}>
              <div className="chart-tip-row"><span className="chart-tip-dot" style={{ background: modelColor.get(seg.model.model) }} /><span className="chart-tip-nm">{modelLabel(seg.model.model)}</span><b>{fmtNanoUsd(seg.model.officialNano)}</b></div>
              <div className="chart-tip-total"><span>{copy.shareOfUse}</span><b>{(seg.share * 100).toFixed(seg.share < 0.1 ? 1 : 0)}%</b></div>
            </div>;
          })()}
        </div>
        <p className="table-scroll-hint" id="models-table-scroll-hint">{copy.tableScrollHint}</p>
        <div className="table-scroll" role="region" tabIndex={0} aria-label={`${copy.tokensAndModels}. ${copy.tableScrollHint}`}><table className="mtable"><thead><tr><th>{copy.model}</th><th className="tnum">{copy.requests}</th><th className="tnum">{copy.inputShort}</th><th className="tnum">{copy.outputShort}</th><th className="tnum">{copy.cacheRdShort}</th><th className="tnum">{copy.cacheWrShort}</th><th className="tnum">{copy.officialValueCol}</th><th className="tnum">{copy.chargedCol}</th></tr></thead>
          <tbody>{models.map((model, index) => <tr key={model.model}>
            <td><span className="tkmdl"><span className="tkmdl-dot" style={{ background: MODEL_COLORS[index % MODEL_COLORS.length] }} />{modelLabel(model.model)}</span></td>
            <td className="tnum">{model.requests.toLocaleString(locale)}</td>
            <td className="tnum">{fmtTokens(model.inputTokens)}</td>
            <td className="tnum">{fmtTokens(model.outputTokens)}</td>
            <td className="tnum">{fmtTokens(model.cacheReadTokens)}</td>
            <td className="tnum">{fmtTokens(model.cacheWrite5mTokens + model.cacheWrite1hTokens)}</td>
            <td className="tnum">{fmtNanoUsd(model.officialNano)}</td>
            <td className="tnum mprice">{fmtNanoUsd(model.chargedNano)}</td>
          </tr>)}</tbody></table></div>
      </>}
    </section>

    {ledgerAvailable && <section className="dsec">
      <div className="dsec-head analytics-heading"><div><h2>{copy.usageByKey}</h2><p>{copy.usageByKeySub}</p></div></div>
      <div className="ubreak-sum">
        <div><span className="dlabel">{copy.keysCount}</span><b>{keyRows.length}</b></div>
        <div><span className="dlabel">{copy.visibleCharges}</span><b>{charges.length}</b></div>
        <div><span className="dlabel">{copy.officialValueCol}</span><b>{formatNanoUsd(sampledOfficialNano)}</b></div>
        <div><span className="dlabel">{copy.chargedCol}</span><b>{formatNanoUsd(sampledChargedNano)}</b></div>
      </div>
      <p className="table-scroll-hint">{copy.tableScrollHint}</p>
      <div className="table-scroll" role="region" tabIndex={0} aria-label={`${copy.usageByKey}. ${copy.tableScrollHint}`}><table className="mtable"><thead><tr><th>{copy.apiKey}</th><th className="tnum">{copy.visibleCharges}</th><th className="tnum">{copy.discount}</th><th className="tnum">{copy.valueColumn}</th><th className="tnum">{copy.officialValueCol}</th><th className="tnum">{copy.chargedCol}</th></tr></thead>
        <tbody>{keyRows.length === 0 ? <tr><td colSpan={6} className="empty-cell">{copy.noChargesPeriod}</td></tr> : keyRows.map((row) => <tr key={row.key}>
          <td><code>{row.key === "__system__" ? copy.systemCharge : row.key}</code></td>
          <td className="tnum">{row.count}</td>
          <td className="tnum">{discount}%</td>
          <td className="tnum"><span className="ubadge">{formatMultiplier(multiplierBp)}</span></td>
          <td className="tnum">{formatNanoUsd(officialNanoFromCharged(row.netNano, multiplierBp))}</td>
          <td className="tnum mprice">{formatNanoUsd(row.netNano)}</td>
        </tr>)}</tbody></table></div>
    </section>}

    {ledgerAvailable && <LedgerHistory ledger={ledger} />}
  </section>;
}

// История ledger сгруппирована по дням: компактные строки-дни (кол-во запросов + сумма), каждая
// раскрывается в отдельные списания. Топапы/коррекции — отдельными выделенными строками. Так вместо
// «вечного полотна» из сотен per-request строк видно читаемую сводку, а детали — по клику.
function LedgerHistory({ ledger }: { ledger: LedgerEntry[] }) {
  const copy = useDashboardCopy();
  const { language } = useI18n();
  const locale = language === "ru" ? "ru-RU" : "en-US";
  if (ledger.length === 0) return <section className="dsec"><h2>{copy.transactions}</h2><div className="empty-box">{copy.noLedger}</div></section>;

  const groups = new Map<number, { day: number; charges: LedgerEntry[]; events: LedgerEntry[] }>();
  for (const entry of ledger) {
    const day = startOfDay(ledgerMs(entry.timestamp));
    const group = groups.get(day) ?? { day, charges: [], events: [] };
    if (entry.kind === "charge") group.charges.push(entry); else group.events.push(entry);
    groups.set(day, group);
  }
  const days = [...groups.values()].sort((a, b) => b.day - a.day);
  const CAP = 50;

  return <section className="dsec"><h2>{copy.transactions}</h2>
    <div className="txh">
      {days.map((group) => {
        const chargeNano = group.charges.reduce((sum, entry) => sum + BigInt(entry.amountNano), 0n);
        return <div className="txh-day" key={group.day}>
          <div className="txh-date">{new Date(group.day).toLocaleDateString(locale, { weekday: "short", month: "short", day: "numeric", year: "numeric" })}</div>
          {group.events.map((entry) => <div className={`txh-ev ${entry.kind}`} key={entry.id}>
            <span className={`pill ${entry.kind === "topup" ? "pill-good" : "pill-soft"}`}>{entry.kind === "topup" ? copy.topupType : copy.adjustType}</span>
            <span className="txh-ev-ref">{entry.reference ?? "—"}</span>
            <span className="txh-ev-amt">{entry.kind === "topup" ? "+" : ""}{formatNanoUsdSmart(BigInt(entry.amountNano))}</span>
          </div>)}
          {group.charges.length > 0 && <details className="txh-charges">
            <summary><span className="txh-sum-l"><span className="txh-ic" aria-hidden="true">▸</span>{interpolate(copy.apiRequestsN, { n: group.charges.length })}</span><span className="txh-sum-amt">−{formatNanoUsdSmart(chargeNano)}</span></summary>
            <div className="txh-list">
              {group.charges.slice(0, CAP).map((entry) => <div className="txh-row" key={entry.id}>
                <span className="txh-time">{new Date(ledgerMs(entry.timestamp)).toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit", second: "2-digit" })}</span>
                <code className="txh-key">{entry.keyMasked ?? "—"}</code>
                <span className="txh-ref">{entry.reference ?? "—"}</span>
                <span className="txh-amt">{formatNanoUsdSmart(BigInt(entry.amountNano))}</span>
              </div>)}
              {group.charges.length > CAP && <div className="txh-more">{interpolate(copy.moreRows, { n: group.charges.length - CAP })}</div>}
            </div>
          </details>}
        </div>;
      })}
    </div>
  </section>;
}

// Мелкие суммы (суб-цент) не округляем в "$0" — показываем честно до значащих знаков.
function formatNanoUsdSmart(value: bigint): string {
  if (value === 0n) return "$0.00";
  if (absoluteBigInt(value) >= 10_000_000n) return formatNanoUsd(value, 2, 2);
  return formatNanoUsd(value, 0, 9);
}

function startOfDay(ms: number): number { const date = new Date(ms); date.setHours(0, 0, 0, 0); return date.getTime(); }
function ledgerMs(timestamp: string): number { const numeric = Number(timestamp); return numeric < 10_000_000_000 ? numeric * 1_000 : numeric; }
function fmtDay(ms: number, locale: string): string { return new Date(ms).toLocaleDateString(locale, { month: "numeric", day: "numeric" }); }

// «Красивая» шкала оси Y на целых нано-USD. В number переводятся только ограниченные отношения для CSS.
function niceNanoScale(max: bigint): { max: bigint; step: bigint; divisions: number } {
  const divisions = 4;
  if (max <= 0n) return { max: NANO_PER_USD, step: NANO_PER_USD / 4n, divisions };
  const rough = (max + BigInt(divisions) - 1n) / BigInt(divisions);
  const magnitude = 10n ** BigInt(Math.max(0, rough.toString().length - 1));
  const candidates = [magnitude, 2n * magnitude, 5n * magnitude, 10n * magnitude];
  const step = candidates.find((candidate) => candidate >= rough) ?? 10n * magnitude;
  return { max: step * BigInt(divisions), step, divisions };
}
function formatAxisNanoUsd(value: bigint): string {
  if (value <= 0n) return "$0";
  if (value >= NANO_PER_USD) return formatNanoUsd(value, 0, 1);
  if (value >= 10_000_000n) return formatNanoUsd(value, 0, 2);
  if (value >= 100_000n) return formatNanoUsd(value, 0, 4);
  return formatNanoUsd(value, 0, 9);
}

// Палитра сегментов по моделям — средние тона, читаются и на светлой, и на тёмной теме.
const MODEL_COLORS = ["#3767f0", "#7c5cff", "#12a594", "#e0913a", "#d6455d", "#8b8f9a"];
function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toLocaleString("en-US", { maximumFractionDigits: 2 })}M`;
  if (n >= 1_000) return `${(n / 1_000).toLocaleString("en-US", { maximumFractionDigits: 1 })}K`;
  return n.toLocaleString("en-US");
}
function fmtNanoUsd(nano: string): string {
  const value = BigInt(nano);
  if (value > 0n && value < 10_000_000n) return "<$0.01";
  return formatNanoUsd(value, 2, 2);
}
function modelLabel(id: string): string {
  const base = id.replace(/^claude-/i, "").replace(/-\d{8}$/, "");
  const words: string[] = []; const nums: string[] = [];
  for (const part of base.split("-")) { if (/^\d+$/.test(part)) nums.push(part); else if (part) words.push(part[0]!.toUpperCase() + part.slice(1)); }
  return `Claude ${words.join(" ")}${nums.length ? ` ${nums.join(".")}` : ""}`.trim();
}

function TwoFactorCard({ user, onUpdated }: { user: AuthUser; onUpdated(user: AuthUser): void }) {
  const copy = useDashboardCopy();
  const [setup, setSetup] = useState<TotpSetup | null>(null);
  const [code, setCode] = useState("");
  const [disarming, setDisarming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const onCode = (value: string) => { setCode(value.replace(/\D/g, "").slice(0, 6)); setError(null); };
  async function refresh() { const me = await api.me(); onUpdated(me.user); }
  async function beginSetup() {
    setBusy(true); setError(null);
    try { setSetup(await api.totpSetup()); setCode(""); }
    catch (cause) { setError(cause instanceof Error ? cause.message : copy.twoFactorError); }
    finally { setBusy(false); }
  }
  async function confirmEnable() {
    setBusy(true); setError(null);
    try { await api.totpEnable(code); trackProductEvent("Two Factor Enabled"); await refresh(); setSetup(null); setCode(""); }
    catch { setError(copy.twoFactorCodeInvalid); }
    finally { setBusy(false); }
  }
  async function confirmDisable() {
    setBusy(true); setError(null);
    try { await api.totpDisable(code); trackProductEvent("Two Factor Disabled"); await refresh(); setDisarming(false); setCode(""); }
    catch { setError(copy.twoFactorCodeInvalid); }
    finally { setBusy(false); }
  }
  function cancel() { setSetup(null); setDisarming(false); setCode(""); setError(null); }
  const codeRow = (onConfirm: () => void, confirmLabel: string) => <div className="tfa-coderow">
    <input className="set-in tfa-code" inputMode="numeric" autoComplete="one-time-code" maxLength={6} value={code} onChange={(event) => onCode(event.target.value)} placeholder="000000" autoFocus />
    <button className="btn btn-ghost btn-sm" disabled={busy} onClick={cancel}>{copy.cancel}</button>
    <button className="btn btn-primary btn-sm" disabled={busy || code.length !== 6} onClick={onConfirm}>{confirmLabel}</button>
  </div>;
  return <div className="card tfa-card">
    <div className="tfa-head"><b>{copy.twoFactorTitle}</b>{user.totpEnabled ? <span className="pill pill-good">{copy.twoFactorOn}</span> : <span className="pill pill-soft">{copy.twoFactorOff}</span>}</div>
    <p className="p-sub tfa-help">{copy.twoFactorGateHelp}</p>
    <p className="p-sub tfa-recovery">{copy.twoFactorRecoveryHelp}</p>
    {user.totpEnabled
      ? (disarming
        ? <><p className="p-sub">{copy.twoFactorDisableHelp}</p>{codeRow(confirmDisable, copy.twoFactorDisable)}</>
        : <button className="btn btn-ghost btn-sm" onClick={() => { setDisarming(true); setError(null); }}>{copy.twoFactorDisable}</button>)
      : (setup
        ? <div className="tfa-enroll">
            <p className="p-sub tfa-scan">{copy.twoFactorScan}</p>
            <div className="tfa-qr"><Image src={setup.qrDataUrl} width={168} height={168} alt="" unoptimized /></div>
            <div className="tfa-secret"><span>{copy.twoFactorManual}</span><code>{setup.secret}</code><CopyButton value={setup.secret} className="tfa-secret-copy" /></div>
            {codeRow(confirmEnable, copy.twoFactorVerify)}
          </div>
        : <button className="btn btn-primary btn-sm" disabled={busy} onClick={beginSetup}>{copy.enable2fa}</button>)}
    {error && <span className="profile-save-error tfa-error" role="alert">{error}</span>}
  </div>;
}

function Profile({ user, onUpdated }: { user: AuthUser; onUpdated(user: AuthUser): void }) {
  const copy = useDashboardCopy();
  const persistedDisplayName = user.displayName || user.email.split("@")[0];
  const [displayName, setDisplayName] = useState(persistedDisplayName);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const trimmedName = displayName.trim();
  const unchanged = trimmedName === persistedDisplayName;
  async function saveProfile(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!trimmedName || trimmedName.length > 80 || unchanged || saving) return;
    setSaving(true); setSaved(false); setSaveError(null);
    try {
      const result = await api.updateProfile(trimmedName);
      trackProductEvent("Profile Updated");
      onUpdated(result.user); setDisplayName(result.user.displayName); setSaved(true);
      window.setTimeout(() => setSaved(false), 2_000);
    } catch (cause) {
      setSaveError(cause instanceof Error ? cause.message : copy.profileSaveError);
    } finally { setSaving(false); }
  }
  return <section className="panel"><PageHeading eyebrow={copy.navAccount} title={copy.profileTitle} subtitle={copy.profileSubtitle} /><div className="prof-grid"><form className="card" onSubmit={saveProfile}><h2>{copy.profileTitle}</h2><div className="set-row"><label className="set-l" htmlFor="profile-email">{copy.email}</label><input id="profile-email" className="set-in profile-email-input" title={user.email} value={user.email} disabled readOnly /></div><div className="set-row"><label className="set-l" htmlFor="profile-display-name">{copy.displayName}</label><input id="profile-display-name" className="set-in" value={displayName} maxLength={80} autoComplete="name" onChange={(event) => { setDisplayName(event.target.value); setSaved(false); setSaveError(null); }} /></div><div className="set-row profile-id-row"><span className="set-l">{copy.userId}</span><span className="uid-wrap"><input className="set-in" value={user.id} aria-label={copy.userId} disabled readOnly /><CopyButton value={user.id} className="uid-copy-button" /></span></div><p className="p-sub">{copy.supportId}</p><div className="profile-meta"><span className="pill">{user.customerType.toUpperCase()}</span><span className="pill pill-soft">Email {user.emailVerified ? copy.verified : copy.pending}</span></div><div className="prof-save"><button className="btn btn-primary btn-sm" type="submit" disabled={saving || unchanged || trimmedName.length === 0}>{saving ? copy.saving : copy.save}</button>{saved && <span className="set-saved always-visible profile-save-success" role="status">{copy.profileSaved}</span>}{saveError && <span className="profile-save-error" role="alert">{saveError}</span>}</div></form>
    <div className="prof-side"><TwoFactorCard user={user} onUpdated={onUpdated} /></div></div>
  </section>;
}

function SupportPanel() {
  const copy = useDashboardCopy();
  return <section className="panel"><PageHeading eyebrow={copy.supportEyebrow} title={copy.supportTitle} subtitle={copy.supportSubtitle} /><SupportContent /></section>;
}

function PromoPanel({ ledger, ledgerAvailable, ledgerMayBePartial }: { ledger: LedgerEntry[]; ledgerAvailable: boolean; ledgerMayBePartial: boolean }) {
  const copy = useDashboardCopy();
  const { language } = useI18n();
  const localCopy = localDashboardCopy[language];
  const search = useSearchParams();
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<{ usd: string; balance?: string } | null>(null);
  const activations = ledger.filter((entry) => entry.kind !== "charge" && entry.reference?.startsWith("promo:"));

  useEffect(() => {
    const prefill = search.get("promo");
    // URL state is browser-owned and intentionally hydrated after mount.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    if (prefill && /^[A-Za-z0-9]{4,32}$/.test(prefill)) setCode(prefill.toUpperCase());
  }, [search]);

  async function redeem(e: FormEvent) {
    e.preventDefault();
    const clean = code.trim().toUpperCase();
    if (!/^[A-Za-z0-9]{4,32}$/.test(clean)) { setError(copy.promoInvalid); return; }
    setBusy(true); setError(null); setDone(null);
    try {
      const res = await api.redeemPromo(clean);
      trackProductEvent("Promo Redeemed");
      setDone({ usd: res.credited_usd, balance: res.balance });
      setCode("");
    } catch (err) {
      setError(err instanceof ApiError ? err.message : copy.promoInvalid);
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="panel">
      <PageHeading eyebrow={copy.promoEyebrow} title={copy.promoTitle} subtitle={copy.promoSubtitle} />
      <div className="card ref-linkcard">
        {done ? (
          <div className="banner banner-accent">
            {copy.promoAdded} <b>${done.usd}</b>
            {done.balance ? ` · ${done.balance}` : ""}
          </div>
        ) : null}
        {error ? <div className="banner banner-error" role="alert">{error}</div> : null}
        <form className="ref-row" onSubmit={redeem}>
          <label className="ref-code-label" htmlFor="promo-code">{copy.promoInput}</label>
          <input
            id="promo-code"
            className="set-in"
            placeholder={copy.promoInput}
            value={code}
            onChange={(e) => setCode(e.target.value.toUpperCase())}
            maxLength={32}
            autoComplete="off"
            spellCheck={false}
          />
          <button className="btn btn-primary btn-sm" type="submit" disabled={busy}>
            {busy ? "…" : copy.activate}
          </button>
        </form>
      </div>
      {ledgerAvailable && ledgerMayBePartial && <div className="banner">{localCopy.partialLedger}</div>}
      {ledgerAvailable && <section className="dsec promo-history">
        <div className="dsec-head"><h2 id="promo-history-title">{copy.myActivations}</h2></div>
        <div className="table-scroll" role="region" tabIndex={0} aria-label={`${copy.myActivations}. ${copy.tableScrollHint}`}>
          <table className="mtable" aria-labelledby="promo-history-title">
            <thead><tr><th>{copy.date}</th><th>{copy.code}</th><th className="tnum">{copy.reward}</th></tr></thead>
            <tbody>{activations.length === 0 ? <tr><td colSpan={3} className="empty-cell">{copy.noPromos}</td></tr> : activations.map((entry) => {
              const referenceId = entry.reference?.slice("promo:".length) ?? "";
              return <tr key={entry.id}>
                <td data-label={copy.date}>{formatLedgerTime(entry.timestamp, language)}</td>
                <td data-label={copy.code}><span className="promo-ledger-label" title={entry.reference ?? undefined}>{copy.promoCredit}{referenceId ? ` · …${referenceId.slice(-8)}` : ""}</span></td>
                <td className="tnum" data-label={copy.reward}>+{formatNanoUsd(BigInt(entry.amountNano))}</td>
              </tr>;
            })}</tbody>
          </table>
        </div>
      </section>}
    </section>
  );
}


function CopyButton({ value, className, label, copiedLabel }: { value: string; className?: string; label?: string; copiedLabel?: string }) {
  const copyText = useDashboardCopy();
  const [copied, setCopied] = useState(false);
  async function copy() {
    let successful = false;
    try {
      await navigator.clipboard.writeText(value);
      successful = true;
    } catch {
      const fallback = document.createElement("textarea");
      fallback.value = value;
      fallback.setAttribute("readonly", "");
      fallback.style.position = "fixed";
      fallback.style.opacity = "0";
      document.body.appendChild(fallback);
      fallback.select();
      successful = document.execCommand("copy");
      fallback.remove();
    }
    if (!successful) return;
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1_200);
  }
  return <button type="button" className={`btn btn-ghost btn-sm${className ? ` ${className}` : ""}`} onClick={copy}>{copied ? (copiedLabel ?? copyText.copied) : (label ?? copyText.copy)}</button>;
}

function formatLedgerTime(timestamp: string, language: "en" | "ru"): string {
  const numeric = Number(timestamp);
  const milliseconds = numeric < 10_000_000_000 ? numeric * 1_000 : numeric;
  return new Date(milliseconds).toLocaleString(language === "ru" ? "ru-RU" : "en-US");
}

function formatOverviewActivityTime(timestamp: string, language: "en" | "ru"): string {
  return new Date(ledgerMs(timestamp)).toLocaleString(language === "ru" ? "ru-RU" : "en-US", {
    month: "short", day: "numeric", hour: "2-digit", minute: "2-digit",
  });
}

function tierName(copy: DashboardCopy, tier: string): string {
  const names: Record<string, string> = {
    starter: copy.tierStarter, builder: copy.tierBuilder, pro: copy.tierPro, studio: copy.tierStudio, scale: copy.tierScale,
  };
  return names[tier] ?? tier;
}

function interpolate(template: string, values: Record<string, string | number>): string {
  return Object.entries(values).reduce((value, [key, replacement]) => value.replaceAll(`{${key}}`, String(replacement)), template);
}

// --- Партнёрская (фиксированная) скидка по реф-ссылке сейлза ---
// Реферал остаётся b2c, но commerce ставит ему «пол» скидки (referral_floor_bps): фиксированная
// ставка поверх/вместо прогрессивных тиров. Если floor > 0 — дашборд показывает её как партнёрскую,
// а реальная доля оплаты берётся из effectiveMultiplierBp (пол переопределяет тир).
function partnerFloorBps(account: AccountView): number {
  const p = account.pricing;
  return p && p.customerType === "b2c" ? (p.referralFloorBps ?? 0) : 0;
}
function isPartnerRate(account: AccountView): boolean {
  return partnerFloorBps(account) > 0;
}

// --- Скидка → сколько реального Claude API получает клиент ---
// multiplierBp = доля оплаты в базисных пунктах (4000 = платит 40% = скидка 60% = ×2.5 ценности).
function paymentBasisPoints(account: AccountView): bigint {
  const p = account.pricing;
  // Партнёрский пол перекрывает тир: реальная ставка = effectiveMultiplierBp (напр. 500 = платит 5%).
  if (p && p.customerType === "b2c" && (p.referralFloorBps ?? 0) > 0 && p.effectiveMultiplierBp && p.effectiveMultiplierBp > 0) {
    return BigInt(p.effectiveMultiplierBp);
  }
  const bp = p?.multiplierBp ?? account.markupBasisPoints;
  return BigInt(bp && bp > 0 ? bp : 4_000);
}
function discountOf(account: AccountView): number {
  const p = account.pricing;
  if (p && p.customerType === "b2c" && (p.referralFloorBps ?? 0) > 0) {
    return p.effectiveDiscountPercent ?? p.discountPercent;
  }
  if (p) return p.discountPercent;
  const discountBp = bigintMax(0n, BASIS_POINTS - paymentBasisPoints(account));
  return Number(roundDivide(discountBp, 100n));
}
function officialNanoFromCharged(chargedNano: bigint, multiplierBp: bigint): bigint {
  return multiplierBp > 0n ? roundDivide(chargedNano * BASIS_POINTS, multiplierBp) : chargedNano;
}
function roundDivide(numerator: bigint, denominator: bigint): bigint {
  if (denominator <= 0n) throw new Error("denominator must be positive");
  const negative = numerator < 0n;
  const absolute = negative ? -numerator : numerator;
  const rounded = (absolute + denominator / 2n) / denominator;
  return negative ? -rounded : rounded;
}
function formatNanoUsd(value: string | bigint, minimumFractionDigits = 0, maximumFractionDigits = 2): string {
  const nano = typeof value === "bigint" ? value : BigInt(value);
  const negative = nano < 0n;
  const absolute = negative ? -nano : nano;
  const digits = Math.max(0, Math.min(9, maximumFractionDigits));
  const minimum = Math.max(0, Math.min(digits, minimumFractionDigits));
  const quantum = 10n ** BigInt(9 - digits);
  const scaled = (absolute + quantum / 2n) / quantum;
  const units = 10n ** BigInt(digits);
  const whole = scaled / units;
  let fraction = digits > 0 ? (scaled % units).toString().padStart(digits, "0") : "";
  while (fraction.length > minimum && fraction.endsWith("0")) fraction = fraction.slice(0, -1);
  return `${negative ? "-" : ""}$${whole.toLocaleString("en-US")}${fraction ? `.${fraction}` : ""}`;
}
function formatMultiplier(multiplierBp: bigint): string {
  return `×${formatFixedRatio(BASIS_POINTS, multiplierBp, 2)}`;
}
function formatPaymentRate(multiplierBp: bigint): string {
  const cents = roundDivide(multiplierBp * 100n, BASIS_POINTS);
  return `${cents / 100n}.${(cents % 100n).toString().padStart(2, "0")}`;
}
function formatPerDollar(multiplierBp: bigint): string {
  return `$${formatFixedRatio(BASIS_POINTS, multiplierBp, 2)}`;
}
function formatFixedRatio(numerator: bigint, denominator: bigint, fractionDigits: number): string {
  if (denominator <= 0n) return "1";
  const scale = 10n ** BigInt(fractionDigits);
  const scaled = roundDivide(numerator * scale, denominator);
  const whole = scaled / scale;
  const fraction = (scaled % scale).toString().padStart(fractionDigits, "0").replace(/0+$/, "");
  return `${whole.toLocaleString("en-US")}${fraction ? `.${fraction}` : ""}`;
}
function multFromDiscount(discountPercent: number): string {
  return formatMultiplier(BigInt(100 - discountPercent) * 100n);
}
function tierIndexForCumulativeNano(spentNano: bigint): number {
  let index = -1;
  B2C_PRICING_MILESTONES.forEach((milestone, milestoneIndex) => {
    if (spentNano >= BigInt(milestone.spendThresholdNano)) index = milestoneIndex;
  });
  return index;
}
function safeCheckoutUrl(rawUrl: string, provider: CheckoutView["provider"]): string | null {
  try {
    const parsed = new URL(rawUrl);
    const allowedOrigins = CHECKOUT_ORIGINS[provider];
    if (parsed.protocol !== "https:" || parsed.username || parsed.password || !allowedOrigins?.has(parsed.origin)) return null;
    return parsed.href;
  } catch { return null; }
}
function boundedRatio(numerator: bigint, denominator: bigint): number {
  if (denominator <= 0n || numerator <= 0n) return 0;
  const scale = 1_000_000n;
  const bounded = bigintMax(0n, numerator > denominator ? denominator : numerator);
  return Number(bounded * scale / denominator) / Number(scale);
}
function boundedPercent(numerator: bigint, denominator: bigint): number {
  return boundedRatio(numerator, denominator) * 100;
}
function compareBigInt(left: bigint, right: bigint): number { return left < right ? -1 : left > right ? 1 : 0; }
function bigintMax(left: bigint, right: bigint): bigint { return left > right ? left : right; }
function absoluteBigInt(value: bigint): bigint { return value < 0n ? -value : value; }
