"use client";

import Link from "next/link";
import dynamic from "next/dynamic";
import { useRouter } from "next/navigation";
import { Activity, memo, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  api, ApiError, type AccountView, type ApiKeyView, type AuthUser, type LedgerEntry, type UsageView,
} from "@/lib/api";
import { normalizeUsd } from "@/lib/money";
import { useI18n } from "@/components/i18n-provider";
import { dashboardCopy, type DashboardCopy } from "@/lib/dashboard-copy";
import { DOCS_URL } from "@/lib/site-links";
import { trackFirstProductEvent, trackProductEvent } from "@/lib/product-analytics";
import { modelLabel } from "@/lib/model-label";
import { withProvisioningRetry } from "@/lib/provisioning-retry";
import { dashboardHref, parseDashboardSection, type DashboardSection } from "./dashboard-route";
import { clearDashboardShellCache, readDashboardShellCache, writeDashboardShellCache } from "./shell-cache";
import { DashboardLoading } from "./dashboard-loading";
import { DashboardScrim, DashboardSidebar, DashboardTopBar } from "./dashboard-shell";

// Ленивые разделы мемоизированы: скрытые (Activity) секции не должны перерисовываться
// от каждого изменения dataPending/dataErrors — только от изменения собственных props.
const ApiKeys = memo(dynamic(() => import("./sections/api-keys").then((module) => module.ApiKeys)));
const Credits = memo(dynamic(() => import("./sections/credits").then((module) => module.Credits)));
const Usage = memo(dynamic(() => import("./sections/usage").then((module) => module.Usage)));
const SupportPanel = memo(dynamic(() => import("./sections/support-panel").then((module) => module.SupportPanel)));
const Profile = memo(dynamic(() => import("./sections/profile").then((module) => module.Profile)));
const PromoPanel = memo(dynamic(() => import("./sections/promo-panel").then((module) => module.PromoPanel)));

type Section = DashboardSection;
type OptionalDataSource = "keys" | "ledger" | "usage";

const NANO_PER_USD = 1_000_000_000n;

// Один in-flight запрос account на всё время жизни вкладки: focus-обновление и
// повторные загрузки делят один промис вместо параллельных копий одного fetch.
let accountRequest: Promise<AccountView> | null = null;
function fetchAccount(): Promise<AccountView> {
  accountRequest ??= api.account().finally(() => { accountRequest = null; });
  return accountRequest;
}
const localDashboardCopy = {
  en: {
    logoutError: "Logout failed. Your server session is still active; please try again.",
    loggingOut: "Logging out…",
    policyActive: "Policy active",
    policySyncing: "Policy syncing",
    policyUnavailable: "Policy details unavailable",
    paidBalance: "Paid balance",
    bonusBalance: "Welcome bonus",
    fundingUnavailable: "Funding split is not available until reconciliation completes.",
    pricingByRule: "Provider and model rules",
    providers: "Providers",
    availableModels: "Available models",
    policyTerms: "Each request uses its provider/model rule; there is no universal balance conversion.",
  },
  ru: {
    logoutError: "Не удалось выйти. Серверная сессия всё ещё активна; повторите попытку.",
    loggingOut: "Выходим…",
    policyActive: "Политика активна",
    policySyncing: "Политика синхронизируется",
    policyUnavailable: "Детали политики недоступны",
    paidBalance: "Оплаченный баланс",
    bonusBalance: "Приветственный бонус",
    fundingUnavailable: "Разбивка средств появится после завершения сверки.",
    pricingByRule: "Правила провайдеров и моделей",
    providers: "Провайдеры",
    availableModels: "Доступные модели",
    policyTerms: "Каждый запрос тарифицируется по правилу провайдера или модели; единого пересчёта баланса нет.",
  },
} as const;

function useDashboardCopy(): DashboardCopy {
  const { language } = useI18n();
  return dashboardCopy[language];
}

export function Dashboard() {
  const router = useRouter();
  const { language, setLanguage } = useI18n();
  const copy = dashboardCopy[language];
  const localCopy = localDashboardCopy[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  // Одноразовое чтение ?view из window.location (паттерн PromoPanel): подписка
  // useSearchParams заставляет Next перерендеривать маршрут на каждый pushState,
  // хотя активный раздел дальше живёт в состоянии компонента. Расхождения SSR/клиент
  // нет: пока loading=true, рендер (DashboardLoading) от section не зависит.
  const [section, setSection] = useState<Section>(() => parseDashboardSection(
    typeof window === "undefined" ? null : new URLSearchParams(window.location.search).get("view"),
  ));
  // Посещённые разделы не размонтируем (Activity hidden): поиск, фильтры и скролл переживают
  // переключение вкладок. Ленивые dynamic()-импорты по-прежнему грузятся только при первом визите.
  const [visitedSections, setVisitedSections] = useState<ReadonlySet<Section>>(() => new Set([section]));
  const [policyNow] = useState(() => Date.now());
  // SWR-снапшот прошлого удачного ответа me+account (sessionStorage). ВАЖНО: в начальные
  // состояния user/account/loading его не подставляем — первый клиентский кадр обязан
  // совпасть с SSR (спиннер), иначе hydration mismatch. Снапшот применяется в эффекте
  // маунта сразу после гидрации, дальше тихая ревалидация подтягивает свежее.
  const [shellCache] = useState(() => readDashboardShellCache());
  const [user, setUser] = useState<AuthUser | null>(null);
  const [account, setAccount] = useState<AccountView | null>(null);
  const [keys, setKeys] = useState<ApiKeyView[]>([]);
  const [ledger, setLedger] = useState<LedgerEntry[]>([]);
  const [usage, setUsage] = useState<UsageView | null>(null);
  const [dataErrors, setDataErrors] = useState<Partial<Record<OptionalDataSource, true>>>({});
  const [dataPending, setDataPending] = useState<Record<OptionalDataSource, boolean>>({ keys: true, ledger: true, usage: true });
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  // Счётчик подряд идущих падений основной загрузки — двигает backoff авторетрая.
  const [loadFailures, setLoadFailures] = useState(0);
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
        const result = await withProvisioningRetry(() => api.apiKeys());
        if (lifecycle !== lifecycleGeneration.current || request !== optionalRequestGeneration.current[source]) return;
        setKeys(result.keys);
      } else if (source === "ledger") {
        const result = await withProvisioningRetry(() => api.ledger(100));
        if (lifecycle !== lifecycleGeneration.current || request !== optionalRequestGeneration.current[source]) return;
        setLedger(result.entries);
        if (result.entries.some((entry) => entry.kind === "topup")) trackFirstProductEvent("topup", "First Top Up", { detected_in: "dashboard" });
        if (result.entries.some((entry) => entry.kind === "charge")) trackFirstProductEvent("api_usage", "First API Usage", { detected_in: "dashboard" });
      } else {
        const result = await withProvisioningRetry(() => api.usage("30d"));
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

  const load = useCallback(async (options?: { silent?: boolean }) => {
    const lifecycle = ++lifecycleGeneration.current;
    // Тихий ретрай (фоновое восстановление) не трогает спиннер и текст ошибки,
    // чтобы экран не мигал между карточкой ошибки и загрузкой.
    const silent = options?.silent === true;
    if (!silent) {
      setLoading(true);
      setError(null);
    }
    try {
      // Optional sources стартуют одновременно с me/account: их данные не зависят
      // от identity-ответа, а каждый источник ловит собственные ошибки, так что
      // падение одного history-эндпоинта не роняет весь дашборд.
      void Promise.all([retryOptional("keys"), retryOptional("ledger"), retryOptional("usage")]);
      const [identity, accountView] = await Promise.all([api.me(), fetchAccount()]);
      if (lifecycle !== lifecycleGeneration.current) return;
      const { user: current } = identity;
      setUser(current); setAccount(accountView);
      writeDashboardShellCache(current, accountView);
      setError(null);
      setLoadFailures(0);
      setLoading(false);
      if (!analyticsLoaded.current) {
        analyticsLoaded.current = true;
        trackProductEvent("Dashboard Opened", { section: initialSection.current, customer_type: current.customerType });
        trackFirstProductEvent("dashboard", "First Dashboard Open", { customer_type: current.customerType });
      }
    } catch (cause) {
      if (lifecycle !== lifecycleGeneration.current) return;
      // Сессия мертва — снапшот чужих/старых данных не должен мелькнуть при следующем входе.
      if (cause instanceof ApiError && cause.status === 401) { clearDashboardShellCache(); router.replace("/login"); return; }
      setError(cause instanceof Error ? cause.message : dashboardCopy.en.loadError);
      setLoadFailures((current) => current + 1);
    } finally {
      if (lifecycle === lifecycleGeneration.current) setLoading(false);
    }
  }, [retryOptional, router]);

  useEffect(() => {
    document.body.classList.add("app-body");
    if (shellCache) {
      // Гидрация завершена — мгновенно подменяем спиннер снапшотом и дальше тихо ревалидируем.
      setUser(shellCache.user);
      setAccount(shellCache.account);
      setLoading(false);
    }
    queueMicrotask(() => { void load({ silent: shellCache !== null }); });
    return () => { lifecycleGeneration.current += 1; document.body.classList.remove("app-body"); };
  }, [load, shellCache]);

  // Чанки самых ходовых разделов (keys/usage) грузим в простое браузера после
  // первого визита: переключение на них не ждёт сеть. Save Data пропускаем.
  const sectionChunksPrefetched = useRef(false);
  useEffect(() => {
    if (!user || sectionChunksPrefetched.current) return;
    if ((navigator as Navigator & { connection?: { saveData?: boolean } }).connection?.saveData) return;
    sectionChunksPrefetched.current = true;
    const prefetch = () => { void import("./sections/api-keys"); void import("./sections/usage"); };
    if (typeof window.requestIdleCallback === "function") window.requestIdleCallback(prefetch, { timeout: 3000 });
    else window.setTimeout(prefetch, 2000);
  }, [user]);

  // Автовосстановление после временной ошибки (5xx бэкенда, обрыв сети): раньше
  // страница падала в тупиковый экран с кнопкой «Log in» при живой сессии. Теперь
  // переспрашиваем с экспоненциальным backoff'ом 1s → 30s (джиттер ±25%), без лимита
  // попыток — частота ограничена потолком 30s, и страница сама оживает, как только
  // бэкенд поднялся. 401 сюда не доезжает: load() уже увёл на /login.
  useEffect(() => {
    if (user || loadFailures === 0) return;
    const base = Math.min(1000 * 2 ** (loadFailures - 1), 30_000);
    const delay = Math.round(base * (0.75 + Math.random() * 0.5));
    const timer = window.setTimeout(() => { void load({ silent: true }); }, delay);
    return () => window.clearTimeout(timer);
  }, [user, loadFailures, load]);

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
        const fresh = await fetchAccount();
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
    // Удалённый раздел Security: состояние уже смаплено parseDashboardSection,
    // здесь только нормализуем адресную строку (одноразовое чтение, без подписки).
    if (new URLSearchParams(window.location.search).get("view") === "security") {
      window.history.replaceState(null, "", dashboardHref("profile", language));
    }
  }, [language]);

  const logout = useCallback(async () => {
    if (loggingOut) return;
    setLoggingOut(true); setLogoutError(null);
    try { await api.logout(); clearDashboardShellCache(); router.replace("/login"); }
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

  const usableKeys = useMemo(() => keys.filter((key) => isApiKeyUsable(key, policyNow)), [keys, policyNow]);
  const closeSide = useCallback(() => setSideOpen(false), []);
  const openMenu = useCallback(() => setSideOpen(true), []);
  const openCredits = useCallback(() => open("credits"), [open]);
  const refreshKeys = useCallback(() => retryOptional("keys", false), [retryOptional]);

  if (loading) return <DashboardLoading label={copy.loading} />;
  if (!user || !account) {
    // 401 уже ушёл редиректом в load(); ошибка здесь — временная (5xx бэкенда,
    // обрыв сети), поэтому экран восстановления с авторетраем, а не кнопка «Log in»
    // при живой сессии. Без ошибки — настоящее незалогиненное состояние.
    if (error) {
      return <div className="wrap guard ym-hide-content"><div className="auth-card">
        <p>{error}</p>
        <p>{copy.loadRetrying}</p>
        <button className="btn btn-primary" onClick={() => void load()}>{copy.retry}</button>
      </div></div>;
    }
    return <div className="wrap guard ym-hide-content"><div className="auth-card"><p>{copy.loginPrompt}</p><Link className="btn btn-primary" href="/login">{copy.login}</Link></div></div>;
  }

  return <div className="app ym-hide-content">
    <DashboardSidebar
      activeSection={section}
      copy={copy}
      language={language}
      sideOpen={sideOpen}
      user={user}
      loggingOut={loggingOut}
      logoutLabel={localCopy.loggingOut}
      onLanguageChange={setLanguage}
      onNavigate={open}
      onLogout={logout}
    />
    <DashboardScrim open={sideOpen} label={copy.closeMenu} onClose={closeSide} />
    <main className="app-main">
      <DashboardTopBar activeSection={section} account={account} copy={copy} locale={locale} onMenu={openMenu} onOpenCredits={openCredits} />
      <DashboardContent
        section={section}
        copy={copy}
        account={account}
        user={user}
        keys={keys}
        usableKeys={usableKeys}
        ledger={ledger}
        usage={usage}
        dataErrors={dataErrors}
        dataPending={dataPending}
        visitedSections={visitedSections}
        error={error}
        logoutError={logoutError}
        loggingOut={loggingOut}
        onRetry={retryOptional}
        onRetryKeys={refreshKeys}
        onRetryLoad={load}
        onLogout={logout}
        onOpen={open}
        onUserUpdated={setUser}
      />
    </main>
  </div>;
}

type DashboardContentProps = {
  section: Section;
  copy: DashboardCopy;
  account: AccountView;
  user: AuthUser;
  keys: ApiKeyView[];
  usableKeys: ApiKeyView[];
  ledger: LedgerEntry[];
  usage: UsageView | null;
  dataErrors: Partial<Record<OptionalDataSource, true>>;
  dataPending: Record<OptionalDataSource, boolean>;
  visitedSections: ReadonlySet<Section>;
  error: string | null;
  logoutError: string | null;
  loggingOut: boolean;
  onRetry(source: OptionalDataSource, showPending?: boolean): Promise<void>;
  onRetryKeys(): Promise<void>;
  onRetryLoad(): Promise<void>;
  onLogout(): Promise<void>;
  onOpen(section: Section): void;
  onUserUpdated(user: AuthUser): void;
};

function DashboardContent({
  section,
  copy,
  account,
  user,
  keys,
  usableKeys,
  ledger,
  usage,
  dataErrors,
  dataPending,
  visitedSections,
  error,
  logoutError,
  loggingOut,
  onRetry,
  onRetryKeys,
  onRetryLoad,
  onLogout,
  onOpen,
  onUserUpdated,
}: DashboardContentProps) {
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

  return <div className="app-body-in">
    {error && <div className="banner banner-error" role="alert">{error} <button className="btn btn-ghost btn-sm" onClick={() => void onRetryLoad()}>{copy.retry}</button></div>}
    {logoutError && <div className="banner banner-error" role="alert">{logoutError} <button className="btn btn-ghost btn-sm" disabled={loggingOut} onClick={() => void onLogout()}>{copy.retry}</button></div>}
    {sourceNotices.map((notice) => <div className={`banner dashboard-data-notice${notice.pending ? "" : " banner-error"}`} role="status" key={notice.source}><span>{notice.message}</span>{!notice.pending && <button className="btn btn-ghost btn-sm" onClick={() => void onRetry(notice.source)}>{copy.retry}</button>}</div>)}
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
        open={onOpen}
      />
    </Activity>}
    {visitedSections.has("keys") && <Activity mode={section === "keys" ? "visible" : "hidden"}>
      {/* Локальный Suspense: без него первый визит в lazy-раздел suspend'ит всё до
          layout-границы и перемонтирует весь дашборд вместе с сайдбаром. */}
      <Suspense fallback={<KeysSkeleton />}>
        {dataPending.keys && !dataErrors.keys && <KeysSkeleton />}
        {!dataPending.keys && !dataErrors.keys && <ApiKeys keys={keys} onChanged={onRetryKeys} user={user} />}
      </Suspense>
    </Activity>}
    {visitedSections.has("credits") && <Activity mode={section === "credits" ? "visible" : "hidden"}>
      <Suspense fallback={<SectionSkeleton />}>
        <Credits account={account} ledger={ledger} ledgerAvailable={!dataPending.ledger && !dataErrors.ledger} />
      </Suspense>
    </Activity>}
    {visitedSections.has("usage") && <Activity mode={section === "usage" ? "visible" : "hidden"}>
      <Suspense fallback={<UsageSkeleton />}>
        {!usage && dataPending.usage && <UsageSkeleton />}
        {usage && <Usage account={account} keys={keys} ledger={ledger} usage={usage} ledgerAvailable={!dataPending.ledger && !dataErrors.ledger} />}
      </Suspense>
    </Activity>}
    {visitedSections.has("support") && <Activity mode={section === "support" ? "visible" : "hidden"}>
      <Suspense fallback={<SectionSkeleton />}>
        <SupportPanel />
      </Suspense>
    </Activity>}
    {visitedSections.has("profile") && <Activity mode={section === "profile" ? "visible" : "hidden"}>
      <Suspense fallback={<SectionSkeleton />}>
        <Profile user={user} onUpdated={onUserUpdated} />
      </Suspense>
    </Activity>}
    {visitedSections.has("promos") && <Activity mode={section === "promos" ? "visible" : "hidden"}>
      <Suspense fallback={<SectionSkeleton />}>
        <PromoPanel ledger={ledger} ledgerAvailable={!dataPending.ledger && !dataErrors.ledger} ledgerMayBePartial={ledger.length >= 100} />
      </Suspense>
    </Activity>}
  </div>;
}

function KeysSkeleton() {
  return <section className="panel keys-skel" aria-hidden="true">
    <div className="skl skl-page-title" /><div className="skl skl-page-sub" />
    <div className="skl skl-toolbar" />
    <div>{[0, 1, 2, 3].map((row) => <div className="skl skl-row" key={row} />)}</div>
  </section>;
}

function SectionSkeleton() {
  return <section className="panel" aria-hidden="true">
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
  const localCopy = localDashboardCopy[language];
  const policy = account.pricingPolicies?.[0] ?? null;
  const appliedPolicy = policy?.applied ?? null;
  const policyModels = appliedPolicy?.providers.flatMap((provider) => provider.models) ?? [];
  const availableModels = policyModels.filter((model) => model.available);
  const availableProviders = appliedPolicy?.providers.filter((provider) => provider.available) ?? [];
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
  const pricingTitle = appliedPolicy ? localCopy.pricingByRule : localCopy.policyUnavailable;
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
          <span className="overview-rate-chip">{policy?.inSync ? localCopy.policyActive : localCopy.policySyncing}</span>
        </div>
        <div className="overview-balance-main">
          <strong className="overview-balance-number">{normalizeUsd(account.balanceUsd)}</strong>
          <div className="overview-balance-detail">
            {account.funding ? <>
              <p className="overview-balance-value">{localCopy.paidBalance}: <b>{formatNanoUsd(account.funding.balances.paidNano, locale)}</b></p>
              <p className="overview-balance-rate">{localCopy.bonusBalance}: <b>{formatNanoUsd(account.funding.balances.bonusNano, locale)}</b></p>
            </> : <p className="overview-balance-rate">{localCopy.fundingUnavailable}</p>}
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
          <span><small>{localCopy.providers}</small><b>{availableProviders.length}</b></span>
          <span><small>{localCopy.availableModels}</small><b>{availableModels.length}</b></span>
        </div>
      </article>

      <article className="card overview-metric-card overview-milestone-card">
        <div className="overview-card-head"><span className="overview-card-label">{copy.pricingTerms}</span><span className="overview-metric-mark" aria-hidden="true">✓</span></div>
        <strong>{localCopy.pricingByRule}</strong>
        <p>{localCopy.policyTerms}</p>
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
              const activityDetail = entry.model
                ? `${modelLabel(entry.model)}${entry.provider ? ` · ${entry.provider}` : ""}`
                : entry.keyMasked ?? entry.reference ?? copy.accountAdjustment;
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
function absoluteBigInt(value: bigint): bigint { return value < 0n ? -value : value; }
