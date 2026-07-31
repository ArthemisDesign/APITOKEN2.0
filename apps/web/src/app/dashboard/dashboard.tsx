"use client";

import Image from "next/image";
import Link from "next/link";
import dynamic from "next/dynamic";
import { useRouter, useSearchParams } from "next/navigation";
import { Activity, memo, useCallback, useEffect, useMemo, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import {
  api, ApiError, type AccountView, type ApiKeyView, type AuthUser, type LedgerEntry, type UsageView,
} from "@/lib/api";
import { normalizeUsd } from "@/lib/money";
import { useI18n } from "@/components/i18n-provider";
import { ThemeToggle } from "@/components/site-chrome";
import { dashboardCopy, type DashboardCopy } from "@/lib/dashboard-copy";
import { DOCS_URL } from "@/lib/site-links";
import { trackFirstProductEvent, trackProductEvent } from "@/lib/product-analytics";
import { modelLabel } from "@/lib/model-label";
import { FLAT_DISCOUNT_PERCENT } from "@/lib/pricing-tiers";
import { dashboardHref, parseDashboardSection, type DashboardSection } from "./dashboard-route";
import { DashboardLoading } from "./dashboard-loading";

const ApiKeys = dynamic(() => import("./sections/api-keys").then((module) => module.ApiKeys));
const Credits = dynamic(() => import("./sections/credits").then((module) => module.Credits));
const Usage = dynamic(() => import("./sections/usage").then((module) => module.Usage));
const SupportPanel = dynamic(() => import("./sections/support-panel").then((module) => module.SupportPanel));
const Profile = dynamic(() => import("./sections/profile").then((module) => module.Profile));
const PromoPanel = dynamic(() => import("./sections/promo-panel").then((module) => module.PromoPanel));

type Section = DashboardSection;
type OptionalDataSource = "keys" | "ledger" | "usage";

const NANO_PER_USD = 1_000_000_000n;
const BASIS_POINTS = 10_000n;
const localDashboardCopy = {
  en: { logoutError: "Logout failed. Your server session is still active; please try again.", loggingOut: "Logging out…" },
  ru: { logoutError: "Не удалось выйти. Серверная сессия всё ещё активна; повторите попытку.", loggingOut: "Выходим…" },
} as const;

const NAV_ICONS = {
  grid: <><rect x="3" y="3" width="7" height="7" rx="1.5" /><rect x="14" y="3" width="7" height="7" rx="1.5" /><rect x="3" y="14" width="7" height="7" rx="1.5" /><rect x="14" y="14" width="7" height="7" rx="1.5" /></>,
  key: <><circle cx="8" cy="15" r="4.5" /><path d="m11 12 9-9" /><path d="m16 7 3 3" /></>,
  external: <><path d="M14 4h6v6" /><path d="M20 4 11 13" /><path d="M19 14v5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2h5" /></>,
  wallet: <><rect x="3" y="5" width="18" height="14" rx="2" /><path d="M3 10h18" /><path d="M7 15h4" /></>,
  percent: <><path d="M19 5 5 19" /><circle cx="7" cy="7" r="2.5" /><circle cx="17" cy="17" r="2.5" /></>,
  chart: <><path d="M4 20V11" /><path d="M10 20V4" /><path d="M16 20v-6" /><path d="M2 20h20" /></>,
  chat: <><path d="M21 12a8.5 8.5 0 0 1-8.5 8.5c-1.6 0-3.1-.4-4.4-1.2L3 21l1.7-5.1A8.5 8.5 0 1 1 21 12z" /></>,
  user: <><circle cx="12" cy="8" r="4" /><path d="M4 21c1.4-3.7 4.6-6 8-6s6.6 2.3 8 6" /></>,
} as const;
type NavIconId = keyof typeof NAV_ICONS;

function NavIcon({ id }: { id: NavIconId }) {
  return <svg viewBox="0 0 24 24" aria-hidden="true">{NAV_ICONS[id]}</svg>;
}

const navigation: Array<{ section?: Section; label: keyof DashboardCopy; icon: NavIconId; href?: string; group?: keyof DashboardCopy }> = [
  { group: "navStart", section: "overview", label: "navOverview", icon: "grid" },
  { group: "navDevelopers", section: "keys", label: "navKeys", icon: "key" },
  { href: DOCS_URL, label: "navDocs", icon: "external" },
  { group: "navBilling", section: "credits", label: "navTopUp", icon: "wallet" },
  { group: "navGrowth", section: "promos", label: "navPromos", icon: "percent" },
  { group: "navActivity", section: "usage", label: "navUsage", icon: "chart" },
  { group: "navSupportGroup", section: "support", label: "navSupport", icon: "chat" },
  { group: "navAccount", section: "profile", label: "navProfile", icon: "user" },
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
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const [section, setSection] = useState<Section>(() => parseDashboardSection(searchParams.get("view")));
  // Посещённые разделы не размонтируем (Activity hidden): поиск, фильтры и скролл переживают
  // переключение вкладок. Ленивые dynamic()-импорты по-прежнему грузятся только при первом визите.
  const [visitedSections, setVisitedSections] = useState<ReadonlySet<Section>>(() => new Set([section]));
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
  const lastFocusRefreshAt = useRef(0);
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
      const next = parseDashboardSection(new URLSearchParams(window.location.search).get("view"));
      setVisitedSections((current) => (current.has(next) ? current : new Set([...current, next])));
      setSection(next);
    }
    window.addEventListener("popstate", syncSectionFromHistory);
    return () => window.removeEventListener("popstate", syncSectionFromHistory);
  }, []);

  // Тихо переподтягиваем аккаунт при возврате фокуса: партнёрская скидка-«пол» реферала обычно
  // применяется синхронно при регистрации, но если она доехала async-фидом уже после открытия
  // дашборда — так витрина (панель «Партнёрская ставка») обновится без ручной перезагрузки.
  // focus и visibilitychange стреляют вместе на каждом переключении вкладки, поэтому успешные
  // обновления троттлим: чаще, чем раз в 30 секунд, аккаунт не переспрашиваем.
  useEffect(() => {
    let cancelled = false;
    async function refreshAccount() {
      if (document.visibilityState !== "visible") return;
      if (Date.now() - lastFocusRefreshAt.current < 30_000) return;
      try {
        const fresh = await api.account();
        lastFocusRefreshAt.current = Date.now();
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

  const logout = useCallback(async () => {
    if (loggingOut) return;
    setLoggingOut(true); setLogoutError(null);
    try { await api.logout(); router.replace("/login"); }
    catch { setLogoutError(localCopy.logoutError); }
    finally { setLoggingOut(false); }
  }, [loggingOut, router, localCopy]);

  const open = useCallback((next: Section) => {
    setSideOpen(false);
    setVisitedSections((current) => (current.has(next) ? current : new Set([...current, next])));
    setSection(next);
    trackProductEvent("Dashboard Section Viewed", { section: next });
    window.history.pushState(null, "", dashboardHref(next, language));
    window.scrollTo({ top: 0, behavior: "auto" });
  }, [language]);

  // Клики с модификаторами (Cmd/Ctrl/Shift) и среднюю кнопку отдаём браузеру: у ссылки настоящий
  // href, поэтому раздел можно открыть в новой вкладке. Обычный клик остаётся shallow-навигацией.
  function handleSectionNav(event: ReactMouseEvent<HTMLAnchorElement>, next: Section) {
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    open(next);
  }

  const usableKeys = useMemo(() => keys.filter((key) => isApiKeyUsable(key, policyNow)), [keys, policyNow]);

  if (loading) return <DashboardLoading label={copy.loading} />;
  if (!user || !account) return <div className="wrap guard ym-hide-content"><div className="auth-card"><p>{error ?? copy.loginPrompt}</p><Link className="btn btn-primary" href="/login">{copy.login}</Link></div></div>;

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
    <aside className={`side ${sideOpen ? "open" : ""}`} data-lang={language}>
      <Link className="brand side-brand" href="/"><BrandImages />apiToken.sale</Link>
      <nav className="side-nav">
        {navigation.map((item, index) => <div key={`${item.label}-${index}`} className="side-nav-item">
          {item.group && <span className="side-group">{copy[item.group]}</span>}
          {item.href ? <Link className="side-link" href={item.href} target="_blank" rel="noreferrer"><span className="si"><NavIcon id={item.icon} /></span><span>{copy[item.label]}</span></Link> :
            <Link data-dashboard-section={item.section} className={`side-link${section === item.section ? " on" : ""}`} aria-current={section === item.section ? "page" : undefined} href={dashboardHref(item.section!, language)} onClick={(event) => handleSectionNav(event, item.section!)}><span className="si"><NavIcon id={item.icon} /></span><span>{copy[item.label]}</span></Link>}
        </div>)}
      </nav>
      <div className="side-foot">
        <div className="side-tools"><div className="lang"><button className={language === "en" ? "active" : ""} aria-pressed={language === "en"} onClick={() => setLanguage("en")}>EN</button><button className={language === "ru" ? "active" : ""} aria-pressed={language === "ru"} onClick={() => setLanguage("ru")}>RU</button></div><ThemeToggle /></div>
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
      <header className="app-top">
        <div className="app-top-in">
          <button className="app-burger" onClick={() => setSideOpen(true)} aria-label={copy.menu}>☰</button>
          <div className="app-top-h"><div className="app-title">{copy[navigation.find((item) => item.section === section)?.label ?? "navOverview"]}</div></div>
          <div className="app-top-actions">
            <button className="app-top-bal" onClick={() => open("credits")} title={copy.navTopUp}>
              <span className="atb-ic" aria-hidden="true" />
              <span className="atb-label">{copy.creditsLabel}</span>
              <span className={`atb-val${BigInt(account.balanceNano) < 0n ? " atb-neg" : ""}`}>{formatNanoUsd(account.balanceNano, locale)}</span>
            </button>
          </div>
        </div>
      </header>
      <div className="app-body-in">
        {error && <div className="banner banner-error" role="alert">{error} <button className="btn btn-ghost btn-sm" onClick={load}>{copy.retry}</button></div>}
        {logoutError && <div className="banner banner-error" role="alert">{logoutError} <button className="btn btn-ghost btn-sm" disabled={loggingOut} onClick={logout}>{copy.retry}</button></div>}
        {sourceNotices.map((notice) => <div className={`banner dashboard-data-notice${notice.pending ? "" : " banner-error"}`} role="status" key={notice.source}><span>{notice.message}</span>{!notice.pending && <button className="btn btn-ghost btn-sm" onClick={() => void retryOptional(notice.source)}>{copy.retry}</button>}</div>)}
        {visitedSections.has("overview") && <Activity mode={section === "overview" ? "visible" : "hidden"}>
          <Overview
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
          />
        </Activity>}
        {visitedSections.has("keys") && <Activity mode={section === "keys" ? "visible" : "hidden"}>
          {dataPending.keys && !dataErrors.keys && <KeysSkeleton />}
          {!dataPending.keys && !dataErrors.keys && <ApiKeys keys={keys} onChanged={() => retryOptional("keys", false)} user={user} />}
        </Activity>}
        {visitedSections.has("credits") && <Activity mode={section === "credits" ? "visible" : "hidden"}>
          <Credits account={account} ledger={ledger} ledgerAvailable={!dataPending.ledger && !dataErrors.ledger} />
        </Activity>}
        {visitedSections.has("usage") && <Activity mode={section === "usage" ? "visible" : "hidden"}>
          {!usage && dataPending.usage && <UsageSkeleton />}
          {usage && <Usage account={account} keys={keys} ledger={ledger} usage={usage} ledgerAvailable={!dataPending.ledger && !dataErrors.ledger} />}
        </Activity>}
        {visitedSections.has("support") && <Activity mode={section === "support" ? "visible" : "hidden"}>
          <SupportPanel />
        </Activity>}
        {visitedSections.has("profile") && <Activity mode={section === "profile" ? "visible" : "hidden"}>
          <Profile user={user} onUpdated={setUser} />
        </Activity>}
        {visitedSections.has("promos") && <Activity mode={section === "promos" ? "visible" : "hidden"}>
          <PromoPanel ledger={ledger} ledgerAvailable={!dataPending.ledger && !dataErrors.ledger} ledgerMayBePartial={ledger.length >= 100} />
        </Activity>}
      </div>
    </main>
  </div>;
}

function BrandImages() {
  return <><Image className="brand-mark bm-light" src="/assets/logo-mark-light.png" width={24} height={24} alt="" /><Image className="brand-mark bm-dark" src="/assets/logo-mark-dark.png" width={24} height={24} alt="" /></>;
}

function KeysSkeleton() {
  return <section className="panel keys-skel" aria-hidden="true">
    <div className="skl skl-page-title" /><div className="skl skl-page-sub" />
    <div className="skl skl-toolbar" />
    <div>{[0, 1, 2, 3].map((row) => <div className="skl skl-row" key={row} />)}</div>
  </section>;
}

function UsageSkeleton() {
  return <section className="panel" aria-hidden="true">
    <div className="skl skl-page-title" /><div className="skl skl-page-sub" />
    <div className="ov-stats bill4">{[0, 1, 2, 3].map((card) => <div className="skl skl-stat" key={card} />)}</div>
    <div className="skl skl-chart" />
  </section>;
}

type OverviewDataState = "loading" | "unavailable" | "ready";

export const Overview = memo(function Overview({ account, user, usableKeys, totalKeys, keysState, usage, usageState, ledger, ledgerState, open }: {
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
  const officialBalanceNano = useMemo(
    () => officialNanoFromCharged(BigInt(account.balanceNano), multiplierBp),
    [account.balanceNano, multiplierBp],
  );
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
  const recentActivity = useMemo(
    () => ledgerState === "ready"
      ? [...ledger].sort((left, right) => ledgerMs(right.timestamp) - ledgerMs(left.timestamp)).slice(0, 3)
      : [],
    [ledger, ledgerState],
  );
  const pricingTitle = account.pricing?.customerType === "b2b" ? copy.businessAgreement : copy.flatRate;
  const showOnboarding = engineReady && keysState === "ready" && totalKeys === 0;

  let alert: { tone: "danger" | "warning"; title: string; text: string; action: "credits" | "keys" } | null = null;
  if (!engineReady) alert = { tone: "danger", title: copy.apiAccessBlocked, text: copy.engineNotReady, action: "keys" };
  else if (keysState === "ready" && totalKeys > 0 && usableKeys.length === 0) alert = { tone: "warning", title: copy.keysNeedAttentionTitle, text: copy.keysNeedAttention, action: "keys" };
  else if (balanceNano <= 0n) alert = { tone: "danger", title: copy.balanceEmptyTitle, text: copy.balanceEmptyText, action: "credits" };
  else if (lowBalance) alert = { tone: "warning", title: copy.balanceLowTitle, text: copy.balanceLowText, action: "credits" };

  return <section className="panel overview-panel">
    <h1 className="sr-only">{copy.navOverview}</h1>
    {alert && <div className={`overview-alert ${alert.tone}`} role="status">
      <span className="overview-alert-icon" aria-hidden="true">!</span>
      <div><strong>{alert.title}</strong><span>{alert.text}</span></div>
      <button className="btn btn-ghost btn-sm" onClick={() => open(alert.action)}>{alert.action === "credits" ? copy.topUp : copy.manageKeys}</button>
    </div>}

    <div className="overview-primary-grid">
      <article className="card overview-balance-card">
        <div className="overview-card-head">
          <span className="overview-card-label">{copy.platformBalance}</span>
          <span className="overview-rate-chip">{discount}% {copy.discount} · {formatMultiplier(multiplierBp, locale)}</span>
        </div>
        <div className="overview-balance-main">
          <strong className="overview-balance-number">{normalizeUsd(account.balanceUsd)}</strong>
          <div className="overview-balance-detail">
            <p className="overview-balance-value">{copy.worthApproximately} <b>≈ {formatNanoUsd(officialBalanceNano, locale)}</b> {copy.inClaudeApiUsage}</p>
            <p className="overview-balance-rate">{interpolate(copy.payPerOfficialDollar, { rate: formatPaymentRate(multiplierBp) })}</p>
            <div className="overview-card-actions">
              <button className="btn btn-primary btn-sm" onClick={() => open("credits")}>{copy.topUp}</button>
              <button className="btn btn-ghost btn-sm" onClick={() => open("usage")}>{copy.viewUsage}</button>
            </div>
          </div>
        </div>
      </article>

      <article className={`card overview-access-card ${accessTone}`}>
        <div className="overview-card-head">
          <span className="overview-card-label">{copy.apiAccess}</span>
          <span className={`overview-status ${accessTone}`}><i aria-hidden="true" />{accessLabel}</span>
        </div>
        <div className="overview-access-body">
          <div className="overview-access-count">
            <strong className="overview-access-value">{keysState === "ready" ? usableKeys.length : "—"}</strong>
            <span className="overview-access-unit">{copy.usableKeys}</span>
          </div>
          <p>{keyStatusText}</p>
          <button className="btn btn-ghost btn-sm" onClick={() => open("keys")}>{totalKeys > 0 ? copy.manageKeys : copy.getKey}</button>
        </div>
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
        <strong>{usageState === "ready" && usage ? formatNanoUsd(usage.totalOfficialNano, locale) : "—"}</strong>
        <p>{usageState === "loading" ? copy.loadingUsageSummary
          : usageState === "unavailable" || !usage ? copy.usageSummaryUnavailable
            : interpolate(copy.usageChargedAndRequests, { charged: formatNanoUsd(usage.totalChargedNano, locale), requests: usage.requests.toLocaleString(locale) })}</p>
        <button className="link plain-button overview-card-link" onClick={() => open("usage")}>{copy.viewUsage} →</button>
      </article>

      <article className="card overview-metric-card overview-pricing-card">
        <div className="overview-card-head"><span className="overview-card-label">{copy.currentPricing}</span><span className="overview-metric-mark" aria-hidden="true">%</span></div>
        <strong>{pricingTitle}</strong>
        <div className="overview-pricing-facts">
          <span><small>{copy.discount}</small><b>{discount}%</b></span>
          <span><small>{copy.valueMultiplier}</small><b>{formatMultiplier(multiplierBp, locale)}</b></span>
        </div>
      </article>

      <article className="card overview-metric-card overview-milestone-card">
        <div className="overview-card-head"><span className="overview-card-label">{copy.pricingTerms}</span><span className="overview-metric-mark" aria-hidden="true">✓</span></div>
        <strong>{copy.flatTermsTitle}</strong>
        <p>{copy.flatTermsSummary}</p>
        <Link className="link overview-card-link" href={`${DOCS_URL}#pricing`}>{copy.howPricingWorks} →</Link>
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
                <b className={isCharge ? "charge" : isTopup ? "topup" : ""}>{amountPrefix}{formatNanoUsd(absoluteBigInt(amount), locale)}</b>
              </div>;
            })}</div>}
    </section>
  </section>;
});

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


function ledgerMs(timestamp: string): number {
  const numeric = Number(timestamp);
  return numeric < 10_000_000_000 ? numeric * 1_000 : numeric;
}

function formatOverviewActivityTime(timestamp: string, language: "en" | "ru"): string {
  return new Date(ledgerMs(timestamp)).toLocaleString(language === "ru" ? "ru-RU" : "en-US", {
    month: "short", day: "numeric", hour: "2-digit", minute: "2-digit",
  });
}

function interpolate(template: string, values: Record<string, string | number>): string {
  return Object.entries(values).reduce((value, [key, replacement]) => value.replaceAll(`{${key}}`, String(replacement)), template);
}

// B2C платит 50% официальной цены (5000 bp, ×2 ценности); B2B сохраняет договорную ставку.
const FLAT_PAYMENT_BP = BigInt((100 - FLAT_DISCOUNT_PERCENT) * 100);
function paymentBasisPoints(account: AccountView): bigint {
  const pricing = account.pricing;
  if (pricing?.customerType === "b2b" && pricing.multiplierBp > 0) return BigInt(pricing.multiplierBp);
  return FLAT_PAYMENT_BP;
}
function discountOf(account: AccountView): number {
  const pricing = account.pricing;
  if (pricing?.customerType === "b2b") return pricing.discountPercent;
  return FLAT_DISCOUNT_PERCENT;
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
function formatNanoUsd(value: string | bigint, locale: string, minimumFractionDigits = 0, maximumFractionDigits = 2): string {
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
  return `${negative ? "-" : ""}$${whole.toLocaleString(locale)}${fraction ? `.${fraction}` : ""}`;
}
function formatMultiplier(multiplierBp: bigint, locale: string): string {
  return `×${formatFixedRatio(BASIS_POINTS, multiplierBp, 2, locale)}`;
}
function formatPaymentRate(multiplierBp: bigint): string {
  const cents = roundDivide(multiplierBp * 100n, BASIS_POINTS);
  return `${cents / 100n}.${(cents % 100n).toString().padStart(2, "0")}`;
}
function formatFixedRatio(numerator: bigint, denominator: bigint, fractionDigits: number, locale: string): string {
  if (denominator <= 0n) return "1";
  const scale = 10n ** BigInt(fractionDigits);
  const scaled = roundDivide(numerator * scale, denominator);
  const whole = scaled / scale;
  const fraction = (scaled % scale).toString().padStart(fractionDigits, "0").replace(/0+$/, "");
  return `${whole.toLocaleString(locale)}${fraction ? `.${fraction}` : ""}`;
}
function absoluteBigInt(value: bigint): bigint { return value < 0n ? -value : value; }
