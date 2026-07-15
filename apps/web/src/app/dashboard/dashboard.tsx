"use client";

import Image from "next/image";
import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { useCallback, useEffect, useState, type CSSProperties } from "react";
import {
  api, ApiError, type AccountView, type ApiKeyView, type AuthUser, type CheckoutView, type LedgerEntry,
} from "@/lib/api";
import { nanoToUsd, normalizeUsd, wholeUsdError } from "@/lib/money";
import { B2C_PRICING_MILESTONES, formatWholeUsd, pricingMilestoneProgress } from "@/lib/pricing-tiers";
import { useI18n } from "@/components/i18n-provider";
import { ThemeToggle } from "@/components/site-chrome";
import { dashboardCopy, type DashboardCopy } from "@/lib/dashboard-copy";
import { DOCS_URL } from "@/lib/site-links";
import { dashboardHref, parseDashboardSection, type DashboardSection } from "./dashboard-route";

type Section = DashboardSection;

const navigation: Array<{ section?: Section; label: keyof DashboardCopy; icon: string; href?: string; group?: keyof DashboardCopy }> = [
  { group: "navStart", section: "overview", label: "navOverview", icon: "▦" },
  { section: "keys", label: "navKeys", icon: "⚿" },
  { section: "credits", label: "navTopUp", icon: "＋" },
  { group: "navGrowth", section: "refer", label: "navReferral", icon: "◈" },
  { section: "promos", label: "navPromos", icon: "%" },
  { group: "navActivity", section: "usage", label: "navUsage", icon: "◔" },
  { section: "orders", label: "navOrders", icon: "▣" },
  { group: "navAccount", section: "profile", label: "navProfile", icon: "◍" },
  { section: "security", label: "navSecurity", icon: "⛨" },
  { href: DOCS_URL, label: "navDocs", icon: "↗" },
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
  const [section, setSection] = useState<Section>(() => parseDashboardSection(searchParams.get("view")));
  const [user, setUser] = useState<AuthUser | null>(null);
  const [account, setAccount] = useState<AccountView | null>(null);
  const [keys, setKeys] = useState<ApiKeyView[]>([]);
  const [ledger, setLedger] = useState<LedgerEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [sideOpen, setSideOpen] = useState(false);

  const load = useCallback(async () => {
    setError(null);
    try {
      const [{ user: current }, accountView, keyList, ledgerView] = await Promise.all([
        api.me(), api.account(), api.apiKeys(), api.ledger(100),
      ]);
      setUser(current); setAccount(accountView); setKeys(keyList.keys); setLedger(ledgerView.entries);
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
    await api.logout().catch(() => undefined); router.replace("/login"); router.refresh();
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
        <div className="side-user"><span className="side-av">{user.email[0]?.toUpperCase()}</span><div className="side-uinfo"><b>{user.email.split("@")[0]}</b><span>{user.email}</span></div></div>
        <button className="btn btn-ghost btn-sm side-logout" onClick={logout}>{copy.logout}</button>
      </div>
    </aside>
    <button className={`side-scrim ${sideOpen ? "show" : ""}`} onClick={() => setSideOpen(false)} aria-label={copy.closeMenu} />
    <main className="app-main">
      <header className="app-top"><button className="app-burger" onClick={() => setSideOpen(true)} aria-label={copy.menu}>☰</button><div className="app-top-h"><div className="app-title">{copy[navigation.find((item) => item.section === section)?.label ?? "navOverview"]}</div><span className="app-top-email">{user.email}</span></div><button className="btn btn-primary btn-sm" onClick={() => open("credits")}>{copy.topUp}</button></header>
      <div className="app-body-in">
        {error && <div className="banner banner-error">{error} <button className="btn btn-ghost btn-sm" onClick={load}>{copy.retry}</button></div>}
        {section === "overview" && <Overview account={account} keys={activeKeys} ledger={ledger} open={open} />}
        {section === "keys" && <ApiKeys keys={keys} onChanged={load} />}
        {section === "credits" && <Credits account={account} />}
        {section === "usage" && <Usage account={account} ledger={ledger} open={open} />}
        {section === "profile" && <Profile user={user} open={open} />}
        {section === "security" && <Security user={user} onLogout={logout} />}
        {section === "refer" && <ReferralPanel />}
        {section === "promos" && <PromoPanel />}
        {section === "orders" && <OrdersPanel />}
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

function Overview({ account, keys, ledger, open }: { account: AccountView; keys: ApiKeyView[]; ledger: LedgerEntry[]; open(section: Section): void }) {
  const copy = useDashboardCopy();
  const spent = nanoToUsd(account.spentNano);
  const total = nanoToUsd((BigInt(account.balanceNano) + BigInt(account.spentNano)).toString());
  const charged = ledger.filter((entry) => entry.kind === "charge").length;
  const complete = 1 + Number(keys.length > 0) + Number(BigInt(account.spentNano) > 0n);
  return <section className="panel">
    <PageHeading eyebrow={copy.workspace} title={copy.overview} subtitle={copy.overviewSubtitle} />
    <PricingBanner account={account} />
    <div className="ov-lead"><div className="ov-lead-l card"><span className="chip">{copy.nextStep}</span><h2>{keys.length ? copy.connectKey : copy.createKey}</h2><p>{copy.oneKeyAllModels}</p><button className="btn btn-primary btn-sm" onClick={() => open("keys")}>⚿ {keys.length ? copy.manageKeys : copy.getKey}</button></div>
      <div className="ov-lead-r card"><div className="rd-head"><div><b>{copy.accountReadiness}</b><span className="rd-steps">{complete}/3 {copy.stepsComplete}</span></div><span className="rd-pct">{Math.round(complete / 3 * 100)}%</span></div><div className="rd-bar"><div className="rd-bar-fill" style={{ width: `${complete / 3 * 100}%` }} /></div><Readiness label={copy.accountActive} done /><Readiness label={copy.apiKey} done={keys.length > 0} /><Readiness label={copy.firstChargedRequest} done={BigInt(account.spentNano) > 0n} /></div></div>
    <div className="ov-stats"><Stat label={copy.remainingBalance} value={normalizeUsd(account.balanceUsd)} detail={`${total} ${copy.funded}`} /><Stat label={copy.used} value={spent} detail={copy.balanceAfterDiscount} /><Stat label={copy.activeKeys} value={String(keys.length)} detail={copy.issuedCredentials} /><Stat label={copy.recentCharges} value={String(charged)} detail={copy.viewLedger} onClick={() => open("usage")} /></div>
    <div className="ov-tiles ov-tiles-two"><Tile icon="▤" title={copy.topUpBalance} subtitle={copy.addApiBalance} onClick={() => open("credits")} /><Tile icon="◍" title={copy.profileSecurity} subtitle={copy.accountAccess} onClick={() => open("profile")} /></div>
    <div className="card connect-card"><div className="cc-head"><div><span className="cc-eyebrow">{copy.connectEyebrow}</span><h2>{copy.connectWithoutDocs}</h2><p>{copy.compatibleEndpoint}</p></div></div><div className="cc-ep"><span className="cc-ep-l">{copy.anthropicEndpoint}</span><div className="ep-row"><code>https://api.apitoken.sale</code><CopyButton value="https://api.apitoken.sale" /></div></div></div>
  </section>;
}

function Readiness({ label, done }: { label: string; done: boolean }) { const copy = useDashboardCopy(); return <div className="rd-row"><span>{label}</span><span className={`rd-st ${done ? "done" : "todo"}`}>{done ? copy.done : copy.todo}</span></div>; }
function Stat({ label, value, detail, onClick }: { label: string; value: string; detail: string; onClick?: () => void }) { return <div className="ovstat"><span className="dlabel">{label}</span><b className="num">{value}</b>{onClick ? <button className="dtrend link plain-button" onClick={onClick}>{detail}</button> : <span className="dtrend">{detail}</span>}</div>; }
function Tile({ icon, title, subtitle, onClick }: { icon: string; title: string; subtitle: string; onClick(): void }) { return <button className="ov-tile" onClick={onClick}><span className="ovt-ic">{icon}</span><span className="ovt-t"><b>{title}</b><span>{subtitle}</span></span><span className="ovt-a">→</span></button>; }

function PricingBanner({ account }: { account: AccountView }) {
  const copy = useDashboardCopy();
  const pricing = account.pricing;
  if (!pricing) return null;
  if (pricing.customerType === "b2b") return <section className="pricing-banner pricing-banner-business"><div className="pricing-summary"><div><span className="pricing-kicker">{copy.currentPricing}</span><strong>{copy.businessAgreement}</strong></div><div className="pricing-discount"><b>{pricing.discountPercent}%</b><span>{copy.discount}</span></div></div><p>{copy.negotiatedRate}</p></section>;
  const currentIndex = Math.max(0, B2C_PRICING_MILESTONES.findIndex((tier) => tier.code === pricing.tier));
  const currentTier = B2C_PRICING_MILESTONES[currentIndex]!;
  const progress = pricingMilestoneProgress(pricing.tier, pricing.spentNano);
  const trackStyle = { "--tier-progress": `${progress}%` } as CSSProperties;
  return <section className="pricing-banner pricing-banner-milestones">
    <div className="pricing-summary">
      <div><span className="pricing-kicker">{copy.monthlyTierProgress}</span><strong>{tierName(copy, currentTier.code)}</strong></div>
      <div className="pricing-discount"><b>{pricing.discountPercent}%</b><span>{copy.discount}</span></div>
    </div>
    <div className="pricing-milestone-status">
      <div className="pricing-status-item"><span>{copy.thisMonth}</span><strong>{nanoToUsd(pricing.spentNano)}</strong><small>{copy.platformSpend}</small></div>
      {pricing.nextTier ? <div className="pricing-status-item pricing-status-next"><span>{copy.nextMilestone}</span><strong>{interpolate(copy.spendMore, { amount: nanoToUsd(pricing.nextTier.remainingNano) })}</strong><small>{interpolate(copy.unlockTier, { tier: tierName(copy, pricing.nextTier.tier), discount: pricing.nextTier.discountPercent })}</small></div> :
        <div className="pricing-status-item pricing-status-next"><span>{copy.milestonesComplete}</span><strong>{copy.highestTierReached}</strong><small>{copy.tierScale} · {pricing.discountPercent}% {copy.discount}</small></div>}
    </div>
    <div className="pricing-milestone-track" style={trackStyle} aria-label={`${Math.round(progress)}% progress through pricing milestones`}>
      <div className="pricing-track-line" aria-hidden="true"><span /></div>
      <ol className="pricing-milestone-list">
        {B2C_PRICING_MILESTONES.map((tier, index) => {
          const state = index < currentIndex ? "complete" : index === currentIndex ? "current" : "upcoming";
          return <li className={`pricing-milestone ${state}`} key={tier.code}>
            <span className="pricing-milestone-dot" aria-hidden="true">{index < currentIndex ? "✓" : index + 1}</span>
            <div><strong>{tierName(copy, tier.code)}</strong><span>{tier.discountPercent}% {copy.discount}</span><small>{index === 0 ? copy.startingTier : interpolate(copy.atSpend, { amount: formatWholeUsd(tier.platformSpendUsd) })}</small></div>
          </li>;
        })}
      </ol>
    </div>
  </section>;
}

function ApiKeys({ keys, onChanged }: { keys: ApiKeyView[]; onChanged(): Promise<void> }) {
  const copy = useDashboardCopy();
  const { language } = useI18n();
  const [label, setLabel] = useState("");
  const [issued, setIssued] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  async function create() {
    setBusy(true); setError(null);
    try { const created = await api.createApiKey(label.trim() || undefined); setIssued(created.key ?? null); setLabel(""); await onChanged(); }
    catch (cause) { setError(cause instanceof Error ? cause.message : copy.createKeyError); }
    finally { setBusy(false); }
  }
  async function revoke(id: string) {
    if (!window.confirm(copy.revokeConfirm)) return;
    setBusy(true); try { await api.revokeApiKey(id); await onChanged(); } catch (cause) { setError(cause instanceof Error ? cause.message : copy.revokeKeyError); } finally { setBusy(false); }
  }
  const snippet = `curl https://api.apitoken.sale/v1/messages \\\n  -H "x-api-key: YOUR_API_KEY" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{"model":"claude-opus-4-8","max_tokens":1024,"messages":[{"role":"user","content":"Hello"}]}'`;
  return <section className="panel"><PageHeading eyebrow={copy.keysEyebrow} title={copy.keysTitle} subtitle={copy.keysSubtitle} />
    <div className="card ep-card"><span className="cc-ep-l">{copy.apiEndpoint}</span><div className="ep-row"><div><span className="ep-t">{copy.anthropicCompatible}</span><code>https://api.apitoken.sale</code></div><CopyButton value="https://api.apitoken.sale" /></div></div>
    {issued && <div className="card secret-card"><span className="chip">{copy.shownOnce}</span><h2>{copy.copyNewKeyNow}</h2><p>{copy.rawSecretWarning}</p><code>{issued}</code><CopyButton value={issued} /><button className="btn btn-ghost btn-sm" onClick={() => setIssued(null)}>{copy.savedKey}</button></div>}
    <section className="dsec"><div className="dsec-head"><h2>{copy.universalKeys}</h2><div className="key-create"><input className="set-in" value={label} onChange={(event) => setLabel(event.target.value)} maxLength={100} placeholder={copy.optionalLabel} /><button className="btn btn-primary btn-sm" disabled={busy} onClick={create}>＋ {copy.newKey}</button></div></div>{error && <div className="banner banner-error">{error}</div>}
      <div className="keys">{keys.length === 0 ? <div className="empty-box">{copy.noKeys}</div> : keys.map((key) => <div className="keyrow" key={key.id}><code className="kval">{key.keyMasked}</code><div className="kmeta">{key.label || copy.unlabelledKey} · {copy.created} {new Date(key.createdAt).toLocaleDateString(language === "ru" ? "ru-RU" : "en-US")} · {copy.spent} {normalizeUsd(key.spentUsd)}</div><div className="kacts"><span className={`pill ${key.status === "active" ? "" : "pill-soft"}`}>{key.status}</span>{key.status === "active" && <button className="btn btn-ghost btn-sm" disabled={busy} onClick={() => revoke(key.id)}>{copy.revoke}</button>}</div></div>)}</div>
    </section><section className="dsec"><div className="dsec-head"><h2>{copy.quickStart}</h2><CopyButton value={snippet} /></div><pre className="code-card"><code>{snippet}</code></pre></section>
  </section>;
}

function Credits({ account }: { account: AccountView }) {
  const copy = useDashboardCopy();
  const [amount, setAmount] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [checkout, setCheckout] = useState<CheckoutView | null>(null);
  async function start() {
    const validation = wholeUsdError(amount);
    if (validation) { setError(validation); return; }
    setBusy(true); setError(null);
    try {
      const created = await api.createCheckout(amount); setCheckout(created);
      if (created.checkoutUrl) window.location.assign(created.checkoutUrl);
    } catch (cause) { setError(cause instanceof Error ? cause.message : copy.createCheckoutError); }
    finally { setBusy(false); }
  }
  return <section className="panel"><PageHeading eyebrow={copy.keysEyebrow} title={copy.creditsTitle} subtitle={copy.creditsSubtitle} />
    <div className="ov-stats bill3"><Stat label={copy.currentBalance} value={normalizeUsd(account.balanceUsd)} detail={copy.available} /><Stat label={copy.used} value={nanoToUsd(account.spentNano)} detail={copy.balanceAfterDiscount} /><Stat label={copy.reserved} value={nanoToUsd(account.reservedNano)} detail={copy.inFlight} /></div>
    <PricingBanner account={account} />
    <div className="card checkout-card"><div><span className="chip">{copy.cryptoCheckout}</span><h2>{copy.anyWholeAmount}</h2><p className="p-sub">{copy.checkoutHelp}</p></div><div className="checkout-entry"><span className="currency-prefix">$</span><input className="set-in" inputMode="numeric" pattern="[1-9][0-9]*" value={amount} onChange={(event) => setAmount(event.target.value.replace(/\D/g, ""))} placeholder="100" /><button className="btn btn-primary" disabled={busy} onClick={start}>{busy ? copy.creating : copy.continuePayment}</button></div>{error && <div className="auth-msg err">{error}</div>}{checkout && !checkout.checkoutUrl && <div className="banner">{interpolate(copy.checkoutPending, { id: checkout.id, status: checkout.status })}</div>}</div>
  </section>;
}

function Usage({ account, ledger, open }: { account: AccountView; ledger: LedgerEntry[]; open(section: Section): void }) {
  const copy = useDashboardCopy();
  const { language } = useI18n();
  const discount = account.pricing?.discountPercent;
  return <section className="panel"><PageHeading eyebrow={copy.usageEyebrow} title={copy.usageTitle} subtitle={copy.usageSubtitle} />
    <div className="banner">💡 <b>{copy.sessionSavingTitle}</b><span> {copy.sessionSavingText}</span></div>
    <div className="us-cards">
      <div className="card us-route"><div className="us-route-h"><b>{copy.apiRouting}</b><span className="pill">API</span></div><span className="dlabel">{copy.availableBalance}</span><div className="us-bal">{normalizeUsd(account.balanceUsd)}</div><span className="p-sub no-margin">{nanoToUsd(account.spentNano)} {copy.totalCharged}</span><p className="p-sub compact-top">{copy.sameBalance}</p></div>
      <div className="card us-health"><div className="us-route-h"><b>{copy.apiHealth}</b><span className="pill pill-soft">{copy.analyticsPending}</span></div><span className="p-sub no-margin">{copy.healthPending}</span><div className="us-health-row"><div><span className="dlabel">{copy.successRate}</span><b className="num">—</b></div><div><span className="dlabel">{copy.averageLatency}</span><b className="num">—</b></div></div><button className="btn btn-primary btn-sm compact-top" onClick={() => open("credits")}>{copy.topUpBalance}</button></div>
    </div>
    <div className="ov-stats bill4"><Stat label={copy.requests} value="—" detail={copy.analyticsPending} /><Stat label={copy.officialSpend} value="—" detail={copy.perRequestPending} /><Stat label={copy.balanceCharged} value={nanoToUsd(account.spentNano)} detail={copy.afterDiscount} /><Stat label={copy.activeDiscount} value={discount === undefined ? "—" : `${discount}%`} detail={account.pricing?.customerType === "b2b" ? copy.businessRate : copy.currentB2cTier} /></div>
    <section className="dsec"><div className="dsec-head analytics-heading"><div><h2>{copy.usageAnalytics}</h2><p>{copy.chartsPending}</p></div><span className="pill pill-soft">{copy.comingSoon}</span></div><div className="analytics-placeholder"><div><span className="dlabel">{copy.usageTrend}</span><b>{copy.officialSpendOverTime}</b></div><div><span className="dlabel">{copy.modelBreakdown}</span><b>{copy.spendByModel}</b></div></div></section>
    <section className="dsec"><h2>{copy.transactions}</h2><div className="table-scroll"><table className="mtable"><thead><tr><th>{copy.time}</th><th>{copy.type}</th><th>{copy.apiKey}</th><th>{copy.reference}</th><th className="tnum">{copy.amount}</th></tr></thead><tbody>{ledger.length === 0 ? <tr><td colSpan={5} className="empty-cell">{copy.noLedger}</td></tr> : ledger.map((entry) => <tr key={entry.id}><td>{formatLedgerTime(entry.timestamp, language)}</td><td><span className="pill pill-soft">{entry.kind}</span></td><td><code>{entry.keyMasked ?? "—"}</code></td><td>{entry.reference ?? "—"}</td><td className="tnum">{normalizeUsd(entry.amountUsd)}</td></tr>)}</tbody></table></div></section>
  </section>;
}

function Profile({ user, open }: { user: AuthUser; open(section: Section): void }) {
  const copy = useDashboardCopy();
  return <section className="panel"><PageHeading eyebrow={copy.navAccount} title={copy.profileTitle} subtitle={copy.profileSubtitle} /><div className="prof-grid"><div className="card"><h2>{copy.profileTitle}</h2><div className="set-row"><span className="set-l">{copy.email}</span><input className="set-in" value={user.email} disabled readOnly /></div><div className="set-row"><span className="set-l">{copy.displayName}</span><input className="set-in" value={user.email.split("@")[0]} disabled readOnly /></div><div className="set-row"><span className="set-l">{copy.userId}</span><span className="uid-wrap"><input className="set-in" value={user.id} readOnly /><CopyButton value={user.id} /></span></div><p className="p-sub">{copy.supportId}</p><div className="profile-meta"><span className="pill">{user.customerType.toUpperCase()}</span><span className="pill pill-soft">Email {user.emailVerified ? copy.verified : copy.pending}</span></div><div className="prof-save"><button className="btn btn-primary btn-sm" disabled>{copy.save}</button><span className="set-saved always-visible">{copy.profileEditingPending}</span></div></div>
    <div className="prof-side"><div className="card"><div className="tg-head"><b>{copy.telegram}</b><span className="pill pill-soft">{copy.notConnected}</span></div><p className="p-sub">{copy.telegramHelp}</p><button className="btn btn-primary btn-sm" disabled>{copy.connectTelegram}</button><div className="set-row compact-top"><span className="set-l">{copy.botLanguage}</span><select className="set-in" disabled defaultValue="English"><option>{copy.english}</option><option>{copy.russian}</option></select></div><ToggleRow label={copy.lowBalanceAlerts} /><ToggleRow label={copy.paymentNotifications} /><ToggleRow label={copy.weeklyDigest} /><div className="set-row"><span className="set-l">{copy.lowBalanceThreshold}</span><input className="set-in set-in-sm" type="number" value="10" disabled readOnly /></div></div><div className="card"><div className="tg-head"><b>{copy.securityTitle}</b></div><p className="p-sub">{copy.securityHelp}</p><button className="btn btn-ghost btn-sm" onClick={() => open("security")}>{copy.securityTitle} →</button></div></div></div></section>;
}

function Security({ user, onLogout }: { user: AuthUser; onLogout(): Promise<void> }) {
  const copy = useDashboardCopy();
  const browser = typeof navigator === "undefined" ? "Current browser" : navigator.userAgent;
  return <section className="panel"><PageHeading eyebrow={copy.navAccount} title={copy.securityTitle} subtitle={copy.securitySubtitle} /><div className="card"><p className="p-sub no-top-margin">{copy.twoFactorHelp}</p><button className="btn btn-primary btn-sm" disabled>{copy.enable2fa}</button><span className="future-note">{copy.backendRequired}</span></div>{user.passwordEnabled ? <section className="dsec"><h2>{copy.password}</h2><div className="set-card"><div className="set-row"><span className="set-l">{copy.currentPassword}</span><input className="set-in" type="password" disabled /></div><div className="set-row"><span className="set-l">{copy.newPassword}</span><input className="set-in" type="password" disabled /></div><button className="btn btn-primary btn-sm" disabled>{copy.updatePassword}</button><span className="future-note">{copy.passwordPending}</span></div></section> : <section className="dsec"><h2>{copy.oauthAccess}</h2><div className="set-card oauth-access-card"><p className="p-sub no-margin">{copy.oauthAccessText}</p></div></section>}<section className="dsec"><h2>{copy.activeSessions}</h2><div className="set-card"><div className="set-row"><span className="set-l"><b>{copy.thisDevice}</b><br /><span className="p-sub session-agent">{browser}</span></span><span className="obadge">{copy.activeNow}</span></div><button className="btn btn-ghost btn-sm" onClick={onLogout}>{copy.logoutSession}</button></div></section></section>;
}

function ReferralPanel() {
  const copy = useDashboardCopy();
  return <section className="panel"><PageHeading eyebrow={copy.navGrowth} title={copy.referralTitle} subtitle={copy.referralSubtitle} /><div className="card ref-linkcard"><span className="cc-ep-l">{copy.referralLink}</span><div className="ref-row"><input className="set-in" value={copy.availableLater} disabled readOnly /><button className="btn btn-primary btn-sm" disabled>{copy.copyLink}</button></div></div><div className="claim-grid"><div className="card claim-card"><div className="claim-top"><b>{copy.claimUsage}</b><span className="pill pill-soft">{copy.pendingUsage}</span></div><div className="claim-amt">$0.00</div><p>{copy.referralUsageHelp}</p><button className="btn btn-primary btn-sm" disabled>{copy.claimUsageAction}</button></div><div className="card claim-card"><div className="claim-top"><b>{copy.claimUsdt}</b><span className="pill pill-soft">{copy.pendingUsdt}</span></div><div className="claim-amt">$0.00</div><p>{copy.usdtReview}</p><button className="btn btn-ghost btn-sm" disabled>{copy.claimUsdtAction}</button></div></div><div className="ov-stats bill4"><Stat label={copy.joined} value="0" detail={copy.notActive} /><Stat label={copy.paid} value="0" detail={copy.notActive} /><Stat label={copy.confirmedRewards} value="0" detail={copy.notActive} /><Stat label={copy.totalUsageReward} value="$0.00" detail={copy.notActive} /></div><section className="dsec"><h2>{copy.rewardsLog}</h2><div className="empty-box">{copy.noRewards}</div></section></section>;
}

function PromoPanel() {
  const copy = useDashboardCopy();
  return <section className="panel"><PageHeading eyebrow={copy.navGrowth} title={copy.promoTitle} subtitle={copy.promoSubtitle} /><div className="card ref-linkcard"><div className="ref-row"><input className="set-in" placeholder="CS-XXXX-XXXX-XXXX" disabled /><button className="btn btn-primary btn-sm" disabled>{copy.activate}</button></div><span className="future-note">{copy.promoPending}</span></div><section className="dsec"><h2>{copy.myActivations}</h2><div className="table-scroll"><table className="mtable"><thead><tr><th>{copy.code}</th><th>{copy.reward}</th><th>{copy.date}</th></tr></thead><tbody><tr><td colSpan={3} className="empty-cell">{copy.noPromos}</td></tr></tbody></table></div></section></section>;
}

function OrdersPanel() {
  const copy = useDashboardCopy();
  return <section className="panel"><PageHeading eyebrow={copy.navActivity} title={copy.ordersTitle} subtitle={copy.ordersSubtitle} /><div className="table-scroll"><table className="mtable"><thead><tr><th>{copy.order}</th><th>{copy.date}</th><th>{copy.description}</th><th className="tnum">{copy.amount}</th><th>{copy.status}</th></tr></thead><tbody><tr><td colSpan={5} className="empty-cell">{copy.noOrders}</td></tr></tbody></table></div></section>;
}

function ToggleRow({ label }: { label: string }) {
  return <label className="tgl-row disabled-control"><span>{label}</span><span className="tgl" /></label>;
}

function CopyButton({ value }: { value: string }) {
  const copyText = useDashboardCopy();
  const [copied, setCopied] = useState(false);
  async function copy() { await navigator.clipboard.writeText(value); setCopied(true); window.setTimeout(() => setCopied(false), 1_200); }
  return <button className="btn btn-ghost btn-sm" onClick={copy}>{copied ? copyText.copied : copyText.copy}</button>;
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
  return Object.entries(values).reduce((value, [key, replacement]) => value.replace(`{${key}}`, String(replacement)), template);
}
