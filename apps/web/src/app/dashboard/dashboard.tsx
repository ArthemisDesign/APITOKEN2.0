"use client";

import Image from "next/image";
import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { useCallback, useEffect, useState, type CSSProperties, type FormEvent } from "react";
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
import { dashboardHref, parseDashboardSection, type DashboardSection } from "./dashboard-route";

type Section = DashboardSection;
type KeyStatusFilter = "active" | "disabled" | "all";

const NANO_PER_USD = 1_000_000_000n;
const BASIS_POINTS = 10_000n;
const CHECKOUT_ORIGINS: Record<CheckoutView["provider"], ReadonlySet<string>> = {
  cryptomus: new Set(["https://pay.cryptomus.com"]),
};

const localDashboardCopy = {
  en: {
    logoutError: "Logout failed. Your server session is still active; please try again.", loggingOut: "Logging out…",
    invalidCheckoutUrl: "The payment provider returned an unsafe checkout address. Payment was not opened.",
    invalidWholeUsd: "Enter a positive whole USD amount using digits only, without decimals, signs, separators, or leading zeros.",
    editLabel: "Rename", saveLabel: "Save", cancelLabel: "Cancel", labelRequired: "Enter a label before saving.", renameKeyError: "Unable to rename API key",
    filterLabel: "Filter API keys", activeFilter: "Active", disabledFilter: "Disabled", allFilter: "All",
    noActiveKeys: "No active API keys.", noDisabledKeys: "No disabled API keys.", activeStatus: "Active", disabledStatus: "Disabled",
    partialLedger: "Showing only the latest 100 ledger entries. Usage, key, transaction, and top-up totals based on this list may be incomplete.",
    topupNextRemaining: "Add {amount} more to reach {tier} (−{discount}%).",
  },
  ru: {
    logoutError: "Не удалось выйти. Серверная сессия всё ещё активна; повторите попытку.", loggingOut: "Выходим…",
    invalidCheckoutUrl: "Платёжный сервис вернул небезопасный адрес. Страница оплаты не была открыта.",
    invalidWholeUsd: "Введите целую положительную сумму в USD только цифрами: без дробей, знаков, разделителей и ведущих нулей.",
    editLabel: "Переименовать", saveLabel: "Сохранить", cancelLabel: "Отмена", labelRequired: "Введите название перед сохранением.", renameKeyError: "Не удалось переименовать API-ключ",
    filterLabel: "Фильтр API-ключей", activeFilter: "Активные", disabledFilter: "Отключённые", allFilter: "Все",
    noActiveKeys: "Активных API-ключей нет.", noDisabledKeys: "Отключённых API-ключей нет.", activeStatus: "Активен", disabledStatus: "Отключён",
    partialLedger: "Показаны только последние 100 записей журнала. Итоги использования, ключей, операций и пополнений по этому списку могут быть неполными.",
    topupNextRemaining: "Добавьте ещё {amount}, чтобы получить {tier} (−{discount}%).",
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
  const [user, setUser] = useState<AuthUser | null>(null);
  const [account, setAccount] = useState<AccountView | null>(null);
  const [keys, setKeys] = useState<ApiKeyView[]>([]);
  const [ledger, setLedger] = useState<LedgerEntry[]>([]);
  const [usage, setUsage] = useState<UsageView | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [logoutError, setLogoutError] = useState<string | null>(null);
  const [loggingOut, setLoggingOut] = useState(false);
  const [sideOpen, setSideOpen] = useState(false);

  const load = useCallback(async () => {
    setError(null);
    try {
      const [{ user: current }, accountView, keyList, ledgerView, usageView] = await Promise.all([
        // usage() терпимо к отсутствию эндпоинта: пока движок не отдаёт per-model — секция покажет pending.
        api.me(), api.account(), api.apiKeys(), api.ledger(100), api.usage("30d").catch(() => null),
      ]);
      setUser(current); setAccount(accountView); setKeys(keyList.keys); setLedger(ledgerView.entries); setUsage(usageView);
    } catch (cause) {
      if (cause instanceof ApiError && cause.status === 401) { router.replace("/login"); return; }
      setError(cause instanceof Error ? cause.message : copy.loadError);
    } finally { setLoading(false); }
  }, [copy.loadError, router]);

  useEffect(() => {
    document.body.classList.add("app-body");
    const timer = window.setTimeout(() => { void load(); }, 0);
    return () => { window.clearTimeout(timer); document.body.classList.remove("app-body"); };
  }, [load]);

  useEffect(() => {
    function syncSectionFromHistory() {
      setSection(parseDashboardSection(new URLSearchParams(window.location.search).get("view")));
    }
    window.addEventListener("popstate", syncSectionFromHistory);
    return () => window.removeEventListener("popstate", syncSectionFromHistory);
  }, []);

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
    window.history.pushState(null, "", dashboardHref(next));
    window.scrollTo({ top: 0, behavior: "auto" });
  }

  if (loading) return <div className="dashboard-loading"><span className="brand">apiToken.sale</span><p>{copy.loading}</p></div>;
  if (!user || !account) return <div className="wrap guard"><div className="auth-card"><p>{error ?? copy.loginPrompt}</p><Link className="btn btn-primary" href="/login">{copy.login}</Link></div></div>;

  const activeKeys = keys.filter((key) => key.status === "active");
  return <div className="app">
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
          <button className="app-top-bal" onClick={() => open("credits")} title={copy.navTopUp}>
            <span className="atb-ic" aria-hidden="true" />
            <span className="atb-label">{copy.creditsLabel}</span>
            <span className={`atb-val${BigInt(account.balanceNano) < 0n ? " atb-neg" : ""}`}>{formatNanoUsd(account.balanceNano)}</span>
          </button>
        </div>
      </header>
      <div className="app-body-in">
        {error && <div className="banner banner-error">{error} <button className="btn btn-ghost btn-sm" onClick={load}>{copy.retry}</button></div>}
        {logoutError && <div className="banner banner-error">{logoutError} <button className="btn btn-ghost btn-sm" disabled={loggingOut} onClick={logout}>{copy.retry}</button></div>}
        {section === "overview" && <Overview account={account} keys={activeKeys} open={open} />}
        {section === "keys" && <ApiKeys keys={keys} onChanged={load} open={open} user={user} />}
        {section === "credits" && <Credits account={account} ledger={ledger} />}
        {section === "usage" && <Usage account={account} ledger={ledger} usage={usage} />}
        {section === "support" && <SupportPanel />}
        {section === "profile" && <Profile user={user} onUpdated={setUser} onLogout={logout} />}
        {section === "promos" && <PromoPanel />}
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

function Overview({ account, keys, open }: { account: AccountView; keys: ApiKeyView[]; open(section: Section): void }) {
  const copy = useDashboardCopy();
  const multiplierBp = paymentBasisPoints(account);
  const discount = discountOf(account);
  const officialBalanceNano = officialNanoFromCharged(BigInt(account.balanceNano), multiplierBp);
  // Реализованная ценность: сколько официального Claude API уже получено за реально списанное.
  const officialUsedNano = officialNanoFromCharged(BigInt(account.spentNano), multiplierBp);
  const hasSpent = BigInt(account.spentNano) > 0n;
  return <section className="panel">
    <div className="overview-core">
      <article className="card overview-balance-card">
        <span className="eyebrow">{copy.platformBalance}</span>
        <div className="overview-balance-values">
          <div><strong>{normalizeUsd(account.balanceUsd)}</strong><span>{copy.availableOnPlatform}</span></div>
          <span className="overview-value-arrow" aria-hidden="true">→</span>
          <div><strong>≈ {formatNanoUsd(officialBalanceNano)}</strong><span>{copy.officialApiValue}</span></div>
        </div>
        <p>{interpolate(copy.balanceValueAtDiscount, { discount })}</p>
        <button className="btn btn-primary btn-sm" onClick={() => open("credits")}>{copy.topUp}</button>
      </article>
      <div className="overview-actions">
        <article className="card overview-action-card">
          <div><span className="dlabel">{copy.activeKeys}</span><strong>{keys.length}</strong></div>
          <p>{keys.length ? copy.keysReady : copy.noKeysOverview}</p>
          <button className="btn btn-ghost btn-sm" onClick={() => open("keys")}>{keys.length ? copy.manageKeys : copy.getKey}</button>
        </article>
        <article className="card overview-action-card">
          <div><span className="dlabel">{copy.claudeApiReceived}</span><strong>{hasSpent ? `≈ ${formatNanoUsd(officialUsedNano)}` : "—"}</strong></div>
          <p>{hasSpent ? interpolate(copy.receivedFromCharged, { charged: formatNanoUsd(account.spentNano), mult: formatMultiplier(multiplierBp) }) : copy.receivedEmpty}</p>
          <button className="btn btn-ghost btn-sm" onClick={() => open("usage")}>{copy.viewUsage}</button>
        </article>
      </div>
    </div>
    <PricingBanner account={account} />
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

function ApiKeys({ keys, onChanged, open, user }: { keys: ApiKeyView[]; onChanged(): Promise<void>; open(section: Section): void; user: AuthUser }) {
  const copy = useDashboardCopy();
  const { language } = useI18n();
  const localCopy = localDashboardCopy[language];
  const [issued, setIssued] = useState<string | null>(null);
  const [label, setLabel] = useState("");
  const [totpCode, setTotpCode] = useState("");
  const [filter, setFilter] = useState<KeyStatusFilter>("active");
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [renameLabel, setRenameLabel] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  async function create() {
    if (user.totpEnabled && !/^\d{6}$/.test(totpCode)) { setError(copy.twoFactorCodeRequired); return; }
    setBusy(true); setError(null);
    try {
      const created = await api.createApiKey(label.trim() || undefined, user.totpEnabled ? totpCode : undefined);
      setIssued(created.key ?? null);
      setLabel(""); setTotpCode("");
      await onChanged();
    }
    catch (cause) {
      const message = cause instanceof ApiError && (cause.message === "2fa_required" || cause.message === "2fa_invalid")
        ? copy.twoFactorCodeInvalid
        : cause instanceof Error ? cause.message : copy.createKeyError;
      setError(message);
    }
    finally { setBusy(false); }
  }
  async function revoke(id: string) {
    if (!window.confirm(copy.revokeConfirm)) return;
    setBusy(true); setError(null);
    try { await api.revokeApiKey(id); await onChanged(); }
    catch (cause) { setError(cause instanceof Error ? cause.message : copy.revokeKeyError); }
    finally { setBusy(false); }
  }
  function beginRename(key: ApiKeyView) {
    setRenamingId(key.id); setRenameLabel(key.label ?? ""); setError(null);
  }
  function cancelRename() { setRenamingId(null); setRenameLabel(""); }
  async function saveRename(id: string) {
    const nextLabel = renameLabel.trim();
    if (!nextLabel) { setError(localCopy.labelRequired); return; }
    setBusy(true); setError(null);
    try { await api.renameApiKey(id, nextLabel); await onChanged(); cancelRename(); }
    catch (cause) { setError(cause instanceof Error ? cause.message : localCopy.renameKeyError); }
    finally { setBusy(false); }
  }
  const counts = {
    active: keys.filter((key) => key.status === "active").length,
    disabled: keys.filter((key) => key.status === "disabled").length,
    all: keys.length,
  };
  const sortedKeys = [...keys]
    .filter((key) => filter === "all" || key.status === filter)
    .sort((left, right) => Number(left.status !== "active") - Number(right.status !== "active"));
  const emptyMessage = filter === "active" ? localCopy.noActiveKeys : filter === "disabled" ? localCopy.noDisabledKeys : copy.noKeys;
  return <section className="panel"><PageHeading eyebrow={copy.keysEyebrow} title={copy.keysTitle} subtitle={copy.keysSubtitle} />
    {issued && <aside className="card secret-card" aria-labelledby="issued-key-title"><div className="secret-head"><h2 id="issued-key-title">{copy.copyNewKeyNow}</h2><span className="chip">{copy.shownOnce}</span></div><p className="secret-warning">{copy.rawSecretWarning}</p><div className="secret-key-field"><code>{issued}</code><CopyButton value={issued} className="secret-copy" /></div><div className="secret-actions"><Link className="btn btn-primary btn-sm" href={DOCS_URL} target="_blank" rel="noreferrer">{copy.openInDocs}</Link><button className="btn btn-ghost btn-sm" onClick={() => open("support")}>{copy.keysHelpCta}</button><button className="btn btn-ghost btn-sm" onClick={() => setIssued(null)}>{copy.savedKey}</button></div></aside>}
    <section className="dsec"><div className="dsec-head"><h2>{copy.universalKeys}</h2><div className="key-create"><input className="set-in" value={label} onChange={(event) => setLabel(event.target.value)} maxLength={64} placeholder={copy.optionalLabel} />{user.totpEnabled && <input className="set-in tfa-code key-2fa" inputMode="numeric" autoComplete="one-time-code" maxLength={6} value={totpCode} onChange={(event) => { setTotpCode(event.target.value.replace(/\D/g, "").slice(0, 6)); setError(null); }} placeholder={copy.twoFactorCodePlaceholder} aria-label={copy.twoFactorCodeLabel} />}<button className="btn btn-primary btn-sm" disabled={busy} onClick={create}>＋ {copy.newKey}</button></div></div>{error && <div className="banner banner-error">{error}</div>}
      <div className="tc-presets" role="group" aria-label={localCopy.filterLabel}>
        {(["active", "disabled", "all"] as const).map((status) => <button key={status} type="button" className={`tc-preset ${filter === status ? "on" : ""}`} aria-pressed={filter === status} onClick={() => setFilter(status)}><b>{status === "active" ? localCopy.activeFilter : status === "disabled" ? localCopy.disabledFilter : localCopy.allFilter}</b><span>{counts[status]}</span></button>)}
      </div>
      <div className="keys">{sortedKeys.length === 0 ? <div className="empty-box">{emptyMessage}</div> : sortedKeys.map((key) => <div className="keyrow" key={key.id}>
        <div className="kval">{renamingId === key.id ? <div className="key-create"><input className="set-in" value={renameLabel} maxLength={64} autoFocus onChange={(event) => setRenameLabel(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter" && renameLabel.trim()) void saveRename(key.id); if (event.key === "Escape") cancelRename(); }} /><button className="btn btn-primary btn-sm" disabled={busy || !renameLabel.trim()} onClick={() => void saveRename(key.id)}>{localCopy.saveLabel}</button><button className="btn btn-ghost btn-sm" disabled={busy} onClick={cancelRename}>{localCopy.cancelLabel}</button></div> : key.label || copy.unlabelledKey}</div>
        <div className="kmeta"><code>{key.keyMasked}</code><span>· {copy.created} {new Date(key.createdAt).toLocaleDateString(language === "ru" ? "ru-RU" : "en-US")} · {copy.spent} {formatNanoUsd(key.spentNano)}</span></div>
        <div className="kacts">
          <span className={`pill ${key.status === "active" ? "" : "pill-soft"}`}>{key.status === "active" ? localCopy.activeStatus : localCopy.disabledStatus}</span>
          {renamingId !== key.id && <button className="btn btn-ghost btn-sm" disabled={busy} aria-label={`${localCopy.editLabel}: ${key.label || copy.unlabelledKey}`} title={localCopy.editLabel} onClick={() => beginRename(key)}>✎</button>}
          {key.status === "active" ? <><Link className="btn btn-ghost btn-sm" href={DOCS_URL} target="_blank" rel="noreferrer">{copy.docsShort}</Link><button className="btn btn-ghost btn-sm" disabled={busy} onClick={() => revoke(key.id)}>{copy.revoke}</button></> : <button className="btn btn-ghost btn-sm" disabled>{copy.docsShort}</button>}
        </div>
      </div>)}</div>
      <div className="keys-help">
        <span className="keys-help-ic" aria-hidden="true"><svg viewBox="0 0 24 24" width="19" height="19" fill="currentColor"><path d="M22 2 2.5 10.6c-.9.4-.9 1.6.1 1.9l4.6 1.4 1.8 5.6c.3.9 1.4 1.1 2 .4l2.5-2.8 4.7 3.4c.8.6 2 .1 2.2-.9L23.9 3.3C24.1 2.3 23 1.5 22 2ZM9 13.6l8.3-5.7-6.4 6.9-.1 3.4L9 13.6Z" /></svg></span>
        <div className="keys-help-txt"><b>{copy.keysHelpTitle}</b><span>{copy.keysHelpText}</span></div>
        <button className="btn btn-ghost btn-sm" onClick={() => open("support")}>{copy.keysHelpCta}</button>
      </div>
    </section>
  </section>;
}

const TOPUP_PRESETS = [100, 250, 500, 1000] as const;

function Credits({ account, ledger }: { account: AccountView; ledger: LedgerEntry[] }) {
  const copy = useDashboardCopy();
  const { language } = useI18n();
  const localCopy = localDashboardCopy[language];
  const [amount, setAmount] = useState("100");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [checkout, setCheckout] = useState<CheckoutView | null>(null);
  const amountValid = /^[1-9]\d*$/.test(amount);
  const amountValidation = amount === "" || amountValid ? null : localCopy.invalidWholeUsd;
  async function start() {
    if (!amountValid) { setError(localCopy.invalidWholeUsd); return; }
    setBusy(true); setError(null);
    try {
      const created = await api.createCheckout(amount); setCheckout(created);
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
  const isB2c = pricing?.customerType === "b2c";
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

  return <section className="panel"><PageHeading eyebrow={copy.keysEyebrow} title={copy.creditsTitle} subtitle={copy.creditsSubtitle} />
    <div className="credits-stack">
      <div className="ov-stats bill3 tc-stats">
        <div className="ovstat"><span className="dlabel">{copy.currentBalance}</span><b className="num">{normalizeUsd(account.balanceUsd)}</b><span className="dtrend">{BigInt(account.balanceNano) > 0n ? interpolate(copy.valueOfBalance, { value: formatNanoUsd(balanceApiNano) }) : copy.available}</span></div>
        <Stat label={copy.used} value={formatNanoUsd(account.spentNano)} detail={copy.balanceAfterDiscount} />
        <div className="ovstat"><span className="dlabel">{copy.currentTier}</span><b className="num">{isB2c ? (currentIdx >= 0 ? tierName(copy, B2C_PRICING_MILESTONES[currentIdx].code) : copy.noTierYet) : copy.businessRate}</b><span className="dtrend">{discountOf(account)}% {copy.discount} · {formatMultiplier(paymentBasisPoints(account))} {copy.valueMultiplier}</span></div>
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
            <span className="tc-recv-sub">{amountNano <= 0n ? copy.enterAmount : hasTier ? `${copy.inClaudeApi} · ${interpolate(copy.atTier, { tier: reachedTier ? tierName(copy, reachedTier.code) : copy.businessRate, discount })}` : `${copy.inClaudeApi} · ${copy.noDiscountYet}`}</span>
            <div className="tc-recv-meta"><span className="tc-badge">−{discount}%</span><span className="tc-badge tc-badge-soft">{formatMultiplier(topupPaymentBp)} {copy.valueMultiplier}</span></div>
          </div>
        </div>
        {isB2c && amountNano > 0n && reachedTier && BigInt(reachedTier.holdUsd) > 0n &&
          <p className="tc-upgrade"><span className="tc-upgrade-ic" aria-hidden="true">ⓘ</span>{interpolate(copy.topupReach, { tier: tierName(copy, reachedTier.code), discount, hold: formatWholeUsd(reachedTier.holdUsd) })}</p>}
        <p className="tc-explain">{hasTier ? interpolate(copy.perDollar, { mult: formatPerDollar(topupPaymentBp) }) : copy.perDollarNone}</p>
        {isB2c && amountNano > 0n && nextTier && nextTierRemainingNano > 0n && <p className="tc-nudge">↑ {interpolate(localCopy.topupNextRemaining, { amount: formatNanoUsd(nextTierRemainingNano), tier: tierName(copy, nextTier.code), discount: nextTier.discountPercent })}</p>}
        <div className="tc-actions"><button className="btn btn-primary" disabled={busy || !amountValid} onClick={start}>{busy ? copy.creating : copy.continuePayment}</button></div>
        {amountValidation && <div className="auth-msg err" id="topup-amount-error">{amountValidation}</div>}
        {error && <div className="auth-msg err">{error}</div>}{checkout && !checkout.checkoutUrl && <div className="banner">{interpolate(copy.checkoutPending, { id: checkout.id, status: checkout.status })}</div>}
      </div>

      <PricingBanner account={account} />

      {ledgerMayBePartial && <div className="banner">{localCopy.partialLedger}</div>}
      <section className="dsec credits-history"><div className="dsec-head"><h2 id="topup-history-title">{copy.topupHistory}</h2></div>
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
      </section>
    </div>
  </section>;
}


function Usage({ account, ledger, usage }: { account: AccountView; ledger: LedgerEntry[]; usage: UsageView | null }) {
  const copy = useDashboardCopy();
  const { language } = useI18n();
  const localCopy = localDashboardCopy[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const models = usage?.models ?? [];
  const modelOfficialTotal = models.reduce((sum, model) => sum + BigInt(model.officialNano), 0n);

  // Скидка определяет, сколько реального Claude API стоит каждый списанный доллар:
  // клиент платит multiplierBp от официальной цены → официальная ценность = списано × 10000 / multiplierBp.
  const multiplierBp = paymentBasisPoints(account);
  const discount = account.pricing?.discountPercent ?? discountOf(account);
  const netChargedNano = BigInt(account.spentNano);
  const officialReceivedNano = officialNanoFromCharged(netChargedNano, multiplierBp);

  const charges = ledger.filter((entry) => entry.kind === "charge");
  const ledgerMayBePartial = ledger.length >= 100;

  // Стабильный цвет на модель: сначала порядок из агрегата usage.models (совпадает с таблицей ниже),
  // затем модели, встреченные только в ledger. Один и тот же id → один цвет во всех графиках.
  const modelColor = new Map<string, string>();
  const assignColor = (id: string) => { if (!modelColor.has(id)) modelColor.set(id, MODEL_COLORS[modelColor.size % MODEL_COLORS.length]!); };
  for (const model of models) assignColor(model.model);

  // Окно графика — ТЕКУЩИЙ КАЛЕНДАРНЫЙ МЕСЯЦ (1-е … последнее число), как в референсе:
  // «сегодня» оказывается не у правого края, а внутри шкалы. Время читаем один раз при монтировании.
  const [nowMs] = useState(() => Date.now());
  const todayMs = startOfDay(nowMs);
  const monthAnchor = new Date(nowMs);
  const mYear = monthAnchor.getFullYear(), mMon = monthAnchor.getMonth();
  const daysInMonth = new Date(mYear, mMon + 1, 0).getDate();
  const days = Array.from({ length: daysInMonth }, (_, index) => new Date(mYear, mMon, index + 1).getTime());

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
  const activeDays = series.filter((point) => point.value > 0n).length;
  const windowOfficialNano = series.reduce((sum, point) => sum + point.value, 0n);
  const windowCharges = charges.filter((charge) => { const b = startOfDay(ledgerMs(charge.timestamp)); return b >= days[0]! && b <= days[days.length - 1]!; }).length;
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

  return <section className="panel"><PageHeading eyebrow={copy.usageEyebrow} title={copy.usageTitle} subtitle={copy.usageSubtitle} />
    <div className="banner">💡 <b>{copy.sessionSavingTitle}</b><span> {copy.sessionSavingText}</span></div>
    {ledgerMayBePartial && <div className="banner">{localCopy.partialLedger}</div>}

    <div className="ov-stats bill4">
      <div className="ovstat"><span className="dlabel">{copy.claudeApiReceived}</span><b className="num accent">{formatNanoUsd(officialReceivedNano)}</b><span className="dtrend">{copy.atOfficialPrices}</span></div>
      <Stat label={copy.balanceCharged} value={formatNanoUsd(account.spentNano)} detail={copy.afterDiscount} />
      <div className="ovstat"><span className="dlabel">{copy.activeDiscount}</span><b className="num">{discount}%</b><span className="dtrend">{formatMultiplier(multiplierBp)} {copy.valueMultiplier}</span></div>
      <div className="ovstat"><span className="dlabel">{copy.availableBalance}</span><b className="num">{normalizeUsd(account.balanceUsd)}</b><span className="dtrend">{BigInt(account.balanceNano) > 0n ? interpolate(copy.valueOfBalance, { value: formatNanoUsd(officialNanoFromCharged(BigInt(account.balanceNano), multiplierBp)) }) : copy.available}</span></div>
    </div>

    <div className="usage-graph">
      <div className="uchart">
        <div className="uchart-head"><b>{copy.usageOverTime}</b><span>{copy.chartWindowLabel}</span></div>
        {maxValue === 0n ? <div className="uchart-empty">{copy.noChargesPeriod}</div> : <>
          <div className="uchart-grid">
            <div className="uchart-yaxis">{gridTicks.map((tick, i) => <span key={i}>{formatAxisNanoUsd(tick)}</span>)}</div>
            <div className="uchart-plotwrap">
              <div className="uchart-lines">{gridTicks.map((_, i) => <i key={i} />)}</div>
              <div className="uchart-plot" onMouseLeave={() => setHoverDay(null)}>
                {series.map((point, index) => <div key={point.day} className={`uchart-col${hoverDay === index ? " is-hover" : ""}`} onMouseEnter={() => setHoverDay(index)}>
                  <div className="uchart-col-fill">
                    {point.segs.map((seg) => <div key={seg.id} className="uchart-seg" style={{ height: `${boundedPercent(seg.value, scale.max)}%`, background: modelColor.get(seg.id) }} />)}
                  </div>
                </div>)}
                {hoverDay !== null && series[hoverDay] && series[hoverDay]!.value > 0n && (() => {
                  const point = series[hoverDay]!;
                  const leftPct = Math.min(92, Math.max(8, (hoverDay + 0.5) / days.length * 100));
                  return <div className="chart-tip" style={{ left: `${leftPct}%`, bottom: `${boundedPercent(point.value, scale.max)}%` }}>
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
      </div>
      <div className="usum">
        <span className="usum-t">{copy.periodSummary}</span>
        <div className="usum-row"><span>{copy.officialSpend}</span><b className="accent">{formatNanoUsd(windowOfficialNano)}</b></div>
        <div className="usum-row"><span>{copy.chargeEvents}</span><b>{windowCharges}</b></div>
        <div className="usum-row"><span>{copy.peakDay}</span><b>{peak.value > 0n ? `${fmtDay(peak.day, locale)} · ${formatNanoUsd(peak.value)}` : "—"}</b></div>
        <div className="usum-row"><span>{copy.dailyAverage}</span><b>{activeDays > 0 ? formatNanoUsd(roundDivide(windowOfficialNano, BigInt(activeDays))) : "—"}</b></div>
      </div>
    </div>

    {usage && <section className="dsec">
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
          <div className="mdist" role="img" aria-label={copy.tokensAndModels} onMouseLeave={() => setMdistHover(null)}>
            {mdistPlaced.map((seg, index) => <div key={seg.model.model} className={`mdist-seg${mdistHover === index ? " is-hover" : ""}`} style={{ width: `${seg.share * 100}%`, background: modelColor.get(seg.model.model) }} onMouseEnter={() => setMdistHover(index)} />)}
          </div>
          {mdistHover !== null && mdistPlaced[mdistHover] && (() => {
            const seg = mdistPlaced[mdistHover]!;
            const leftPct = Math.min(92, Math.max(8, seg.center * 100));
            return <div className="chart-tip mdist-tip" style={{ left: `${leftPct}%` }}>
              <div className="chart-tip-row"><span className="chart-tip-dot" style={{ background: modelColor.get(seg.model.model) }} /><span className="chart-tip-nm">{modelLabel(seg.model.model)}</span><b>{fmtNanoUsd(seg.model.officialNano)}</b></div>
              <div className="chart-tip-total"><span>{copy.shareOfUse}</span><b>{(seg.share * 100).toFixed(seg.share < 0.1 ? 1 : 0)}%</b></div>
            </div>;
          })()}
        </div>
        <div className="table-scroll"><table className="mtable"><thead><tr><th>{copy.model}</th><th className="tnum">{copy.requests}</th><th className="tnum">{copy.inputShort}</th><th className="tnum">{copy.outputShort}</th><th className="tnum">{copy.cacheRdShort}</th><th className="tnum">{copy.cacheWrShort}</th><th className="tnum">{copy.officialValueCol}</th><th className="tnum">{copy.chargedCol}</th></tr></thead>
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
    </section>}

    <section className="dsec">
      <div className="dsec-head analytics-heading"><div><h2>{copy.usageByKey}</h2><p>{copy.usageByKeySub}</p></div></div>
      <div className="ubreak-sum">
        <div><span className="dlabel">{copy.keysCount}</span><b>{keyRows.length}</b></div>
        <div><span className="dlabel">{copy.requests}</span><b>{charges.length}</b></div>
        <div><span className="dlabel">{copy.officialValueCol}</span><b>{formatNanoUsd(officialReceivedNano)}</b></div>
        <div><span className="dlabel">{copy.chargedCol}</span><b>{formatNanoUsd(account.spentNano)}</b></div>
      </div>
      <div className="table-scroll"><table className="mtable"><thead><tr><th>{copy.apiKey}</th><th className="tnum">{copy.requests}</th><th className="tnum">{copy.discount}</th><th className="tnum">{copy.valueColumn}</th><th className="tnum">{copy.officialValueCol}</th><th className="tnum">{copy.chargedCol}</th></tr></thead>
        <tbody>{keyRows.length === 0 ? <tr><td colSpan={6} className="empty-cell">{copy.noChargesPeriod}</td></tr> : keyRows.map((row) => <tr key={row.key}>
          <td><code>{row.key === "__system__" ? copy.systemCharge : row.key}</code></td>
          <td className="tnum">{row.count}</td>
          <td className="tnum">{discount}%</td>
          <td className="tnum"><span className="ubadge">{formatMultiplier(multiplierBp)}</span></td>
          <td className="tnum">{formatNanoUsd(officialNanoFromCharged(row.netNano, multiplierBp))}</td>
          <td className="tnum mprice">{formatNanoUsd(row.netNano)}</td>
        </tr>)}</tbody></table></div>
    </section>

    <LedgerHistory ledger={ledger} />
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
    try { await api.totpEnable(code); await refresh(); setSetup(null); setCode(""); }
    catch { setError(copy.twoFactorCodeInvalid); }
    finally { setBusy(false); }
  }
  async function confirmDisable() {
    setBusy(true); setError(null);
    try { await api.totpDisable(code); await refresh(); setDisarming(false); setCode(""); }
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

function Profile({ user, onUpdated, onLogout }: { user: AuthUser; onUpdated(user: AuthUser): void; onLogout(): Promise<void> }) {
  const copy = useDashboardCopy();
  const browser = typeof navigator === "undefined" ? "Current browser" : navigator.userAgent;
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
      onUpdated(result.user); setDisplayName(result.user.displayName); setSaved(true);
      window.setTimeout(() => setSaved(false), 2_000);
    } catch (cause) {
      setSaveError(cause instanceof Error ? cause.message : copy.profileSaveError);
    } finally { setSaving(false); }
  }
  return <section className="panel"><PageHeading eyebrow={copy.navAccount} title={copy.profileTitle} subtitle={copy.profileSubtitle} /><div className="prof-grid"><form className="card" onSubmit={saveProfile}><h2>{copy.profileTitle}</h2><div className="set-row"><label className="set-l" htmlFor="profile-email">{copy.email}</label><input id="profile-email" className="set-in" value={user.email} disabled readOnly /></div><div className="set-row"><label className="set-l" htmlFor="profile-display-name">{copy.displayName}</label><input id="profile-display-name" className="set-in" value={displayName} maxLength={80} autoComplete="name" onChange={(event) => { setDisplayName(event.target.value); setSaved(false); setSaveError(null); }} /></div><div className="set-row profile-id-row"><span className="set-l">{copy.userId}</span><span className="uid-wrap"><input className="set-in" value={user.id} aria-label={copy.userId} disabled readOnly /><CopyButton value={user.id} className="uid-copy-button" /></span></div><p className="p-sub">{copy.supportId}</p><div className="profile-meta"><span className="pill">{user.customerType.toUpperCase()}</span><span className="pill pill-soft">Email {user.emailVerified ? copy.verified : copy.pending}</span></div><div className="prof-save"><button className="btn btn-primary btn-sm" type="submit" disabled={saving || unchanged || trimmedName.length === 0}>{saving ? copy.saving : copy.save}</button>{saved && <span className="set-saved always-visible profile-save-success" role="status">{copy.profileSaved}</span>}{saveError && <span className="profile-save-error" role="alert">{saveError}</span>}</div></form>
    <div className="prof-side"><TwoFactorCard user={user} onUpdated={onUpdated} /></div></div>
    {user.passwordEnabled ? <section className="dsec"><h2>{copy.password}</h2><div className="set-card"><div className="set-row"><span className="set-l">{copy.currentPassword}</span><input className="set-in" type="password" disabled /></div><div className="set-row"><span className="set-l">{copy.newPassword}</span><input className="set-in" type="password" disabled /></div><button className="btn btn-primary btn-sm" disabled>{copy.updatePassword}</button><span className="future-note">{copy.passwordPending}</span></div></section> : <section className="dsec"><h2>{copy.oauthAccess}</h2><div className="set-card oauth-access-card"><p className="p-sub no-margin">{copy.oauthAccessText}</p></div></section>}
    <section className="dsec"><h2>{copy.activeSessions}</h2><div className="set-card"><div className="set-row"><span className="set-l"><b>{copy.thisDevice}</b><br /><span className="p-sub session-agent">{browser}</span></span><span className="obadge">{copy.activeNow}</span></div><button className="btn btn-ghost btn-sm" onClick={onLogout}>{copy.logoutSession}</button></div></section>
  </section>;
}

function SupportPanel() {
  const copy = useDashboardCopy();
  return <section className="panel"><PageHeading eyebrow={copy.supportEyebrow} title={copy.supportTitle} subtitle={copy.supportSubtitle} /><SupportContent /></section>;
}

function PromoPanel() {
  const copy = useDashboardCopy();
  return <section className="panel"><PageHeading eyebrow={copy.navGrowth} title={copy.promoTitle} subtitle={copy.promoSubtitle} /><div className="card ref-linkcard"><div className="ref-row"><input className="set-in" placeholder="CS-XXXX-XXXX-XXXX" disabled /><button className="btn btn-primary btn-sm" disabled>{copy.activate}</button></div><span className="future-note">{copy.promoPending}</span></div><section className="dsec"><h2>{copy.myActivations}</h2><div className="table-scroll"><table className="mtable"><thead><tr><th>{copy.code}</th><th>{copy.reward}</th><th>{copy.date}</th></tr></thead><tbody><tr><td colSpan={3} className="empty-cell">{copy.noPromos}</td></tr></tbody></table></div></section></section>;
}


function CopyButton({ value, className }: { value: string; className?: string }) {
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
  return <button type="button" className={`btn btn-ghost btn-sm${className ? ` ${className}` : ""}`} onClick={copy}>{copied ? copyText.copied : copyText.copy}</button>;
}

function formatLedgerTime(timestamp: string, language: "en" | "ru"): string {
  const numeric = Number(timestamp);
  const milliseconds = numeric < 10_000_000_000 ? numeric * 1_000 : numeric;
  return new Date(milliseconds).toLocaleString(language === "ru" ? "ru-RU" : "en-US");
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

// --- Скидка → сколько реального Claude API получает клиент ---
// multiplierBp = доля оплаты в базисных пунктах (4000 = платит 40% = скидка 60% = ×2.5 ценности).
function paymentBasisPoints(account: AccountView): bigint {
  const bp = account.pricing?.multiplierBp ?? account.markupBasisPoints;
  return BigInt(bp && bp > 0 ? bp : 4_000);
}
function discountOf(account: AccountView): number {
  if (account.pricing) return account.pricing.discountPercent;
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
