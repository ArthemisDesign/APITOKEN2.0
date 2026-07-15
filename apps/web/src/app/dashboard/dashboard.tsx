"use client";

import Image from "next/image";
import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { useCallback, useEffect, useState, type CSSProperties, type FormEvent } from "react";
import {
  api, ApiError, type AccountView, type ApiKeyView, type AuthUser, type CheckoutView, type LedgerEntry, type UsageView,
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
  { group: "navGrowth", section: "promos", label: "navPromos", icon: "%" },
  { group: "navActivity", section: "usage", label: "navUsage", icon: "◔" },
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
  const [usage, setUsage] = useState<UsageView | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
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
    await api.logout().catch(() => undefined); router.replace("/login");
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
        <button className="btn btn-ghost btn-sm side-logout" onClick={logout}>{copy.logout}</button>
      </div>
    </aside>
    <button className={`side-scrim ${sideOpen ? "show" : ""}`} onClick={() => setSideOpen(false)} aria-label={copy.closeMenu} />
    <main className="app-main">
      <header className="app-top"><button className="app-burger" onClick={() => setSideOpen(true)} aria-label={copy.menu}>☰</button><div className="app-top-h"><div className="app-title">{copy[navigation.find((item) => item.section === section)?.label ?? "navOverview"]}</div></div><button className="btn btn-primary btn-sm" onClick={() => open("credits")}>{copy.topUp}</button></header>
      <div className="app-body-in">
        {error && <div className="banner banner-error">{error} <button className="btn btn-ghost btn-sm" onClick={load}>{copy.retry}</button></div>}
        {section === "overview" && <Overview account={account} keys={activeKeys} ledger={ledger} open={open} />}
        {section === "keys" && <ApiKeys keys={keys} onChanged={load} />}
        {section === "credits" && <Credits account={account} ledger={ledger} />}
        {section === "usage" && <Usage account={account} ledger={ledger} usage={usage} />}
        {section === "profile" && <Profile user={user} open={open} onUpdated={setUser} />}
        {section === "security" && <Security user={user} onLogout={logout} />}
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

function Overview({ account, keys, ledger, open }: { account: AccountView; keys: ApiKeyView[]; ledger: LedgerEntry[]; open(section: Section): void }) {
  const copy = useDashboardCopy();
  const fraction = payFraction(account);
  const discount = discountOf(account);
  const officialBalance = nanoNum(account.balanceNano) / fraction;
  const monthlyPricing = account.pricing?.customerType === "b2c" ? account.pricing : null;
  const monthStart = monthlyPricing ? Date.parse(monthlyPricing.monthStart) : null;
  const charged = ledger.filter((entry) => entry.kind === "charge" && (monthStart === null || ledgerMs(entry.timestamp) >= monthStart)).length;
  return <section className="panel">
    <div className="overview-core">
      <article className="card overview-balance-card">
        <span className="eyebrow">{copy.platformBalance}</span>
        <div className="overview-balance-values">
          <div><strong>{normalizeUsd(account.balanceUsd)}</strong><span>{copy.availableOnPlatform}</span></div>
          <span className="overview-value-arrow" aria-hidden="true">→</span>
          <div><strong>≈ {fmtUsd(officialBalance)}</strong><span>{copy.officialApiValue}</span></div>
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
          <div><span className="dlabel">{monthlyPricing ? copy.thisMonth : copy.used}</span><strong>{nanoToUsd(monthlyPricing?.spentNano ?? account.spentNano)}</strong></div>
          <p>{interpolate(copy.chargeEventsThisMonth, { count: charged })}</p>
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
  const pricing = account.pricing;
  if (!pricing) return null;
  if (pricing.customerType === "b2b") return <section className="pricing-banner pricing-banner-business"><div className="pricing-summary"><div><span className="pricing-kicker">{copy.currentPricing}</span><strong>{copy.businessAgreement}</strong></div><div className="pricing-discount"><b>{pricing.discountPercent}%</b><span>{copy.discount}</span><em className="pricing-mult">{multFromDiscount(pricing.discountPercent)} {copy.valueMultiplier}</em></div></div><p>{copy.negotiatedRate}</p></section>;
  const currentIndex = Math.max(0, B2C_PRICING_MILESTONES.findIndex((tier) => tier.code === pricing.tier));
  const currentTier = B2C_PRICING_MILESTONES[currentIndex]!;
  const progress = pricingMilestoneProgress(pricing.tier, pricing.spentNano);
  const trackStyle = { "--tier-progress": `${progress}%` } as CSSProperties;
  return <section className="pricing-banner pricing-banner-milestones">
    <div className="pricing-summary">
      <div><span className="pricing-kicker">{copy.monthlyTierProgress}</span><strong>{tierName(copy, currentTier.code)}</strong></div>
      <div className="pricing-discount"><b>{pricing.discountPercent}%</b><span>{copy.discount}</span><em className="pricing-mult">{multFromDiscount(pricing.discountPercent)} {copy.valueMultiplier}</em></div>
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
            <div><strong>{tierName(copy, tier.code)}</strong><span>{tier.discountPercent}% {copy.discount}</span><em className="pm-mult">{multFromDiscount(tier.discountPercent)} {copy.valueMultiplier}</em><small>{index === 0 ? copy.startingTier : interpolate(copy.atSpend, { amount: formatWholeUsd(tier.platformSpendUsd) })}</small></div>
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

const TOPUP_PRESETS = [20, 50, 100, 200] as const;

function Credits({ account, ledger }: { account: AccountView; ledger: LedgerEntry[] }) {
  const copy = useDashboardCopy();
  const { language } = useI18n();
  const [amount, setAmount] = useState("100");
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

  // Разовое пополнение может РАЗБЛОКИРОВАТЬ более высокий тариф (по тем же порогам расхода):
  // чем больше кладёшь сразу, тем выше скидка → тем больше реального Claude API получаешь.
  const pricing = account.pricing;
  const isB2c = pricing?.customerType === "b2c";
  const currentIdx = pricing?.customerType === "b2c"
    ? Math.max(0, B2C_PRICING_MILESTONES.findIndex((m) => m.code === pricing.tier)) : 0;
  const baseFrac = payFraction(account);
  const amt = Number(amount) || 0;
  const fracForAmount = (a: number) => isB2c
    ? (100 - B2C_PRICING_MILESTONES[Math.max(currentIdx, tierIndexForAmount(a))].discountPercent) / 100
    : baseFrac;
  const unlockedIdx = isB2c ? Math.max(currentIdx, tierIndexForAmount(amt)) : currentIdx;
  const unlockedTier = B2C_PRICING_MILESTONES[unlockedIdx];
  const unlockedDiscount = isB2c ? unlockedTier.discountPercent : discountOf(account);
  const unlockedFrac = fracForAmount(amt);
  const apiValue = amt / unlockedFrac;
  const isUpgrade = isB2c && unlockedIdx > currentIdx;
  const balanceApi = nanoNum(account.balanceNano) / baseFrac;
  const upsell = isB2c && unlockedIdx + 1 < B2C_PRICING_MILESTONES.length ? B2C_PRICING_MILESTONES[unlockedIdx + 1] : null;
  const topups = ledger.filter((entry) => entry.kind === "topup");

  return <section className="panel"><PageHeading eyebrow={copy.keysEyebrow} title={copy.creditsTitle} subtitle={copy.creditsSubtitle} />
    <div className="ov-stats bill3">
      <div className="ovstat"><span className="dlabel">{copy.currentBalance}</span><b className="num">{normalizeUsd(account.balanceUsd)}</b><span className="dtrend">{nanoNum(account.balanceNano) > 0 ? interpolate(copy.valueOfBalance, { value: fmtUsd(balanceApi) }) : copy.available}</span></div>
      <Stat label={copy.used} value={nanoToUsd(account.spentNano)} detail={copy.balanceAfterDiscount} />
      <div className="ovstat"><span className="dlabel">{copy.currentTier}</span><b className="num">{isB2c ? tierName(copy, B2C_PRICING_MILESTONES[currentIdx].code) : copy.businessRate}</b><span className="dtrend">{discountOf(account)}% {copy.discount} · {fmtMult(baseFrac)} {copy.valueMultiplier}</span></div>
    </div>

    <div className="card topup-convert">
      <div className="tc-head"><span className="chip">{copy.cryptoCheckout}</span><h2>{copy.anyWholeAmount}</h2><p className="p-sub">{copy.checkoutHelp}</p></div>
      <div className="tc-body">
        <div className="tc-input">
          <label className="tc-field"><span className="currency-prefix">$</span><input className="set-in" inputMode="numeric" pattern="[1-9][0-9]*" value={amount} onChange={(event) => setAmount(event.target.value.replace(/\D/g, ""))} placeholder="100" /></label>
          <div className="tc-presets" role="group" aria-label={copy.quickAmounts}>{TOPUP_PRESETS.map((preset) => <button key={preset} type="button" className={`tc-preset ${amount === String(preset) ? "on" : ""}`} onClick={() => { setAmount(String(preset)); setError(null); }}><b>${preset}</b><span>{fmtUsd(preset / fracForAmount(preset))}</span></button>)}</div>
        </div>
        <div className="tc-arrow" aria-hidden="true">→</div>
        <div className={`tc-receive ${isUpgrade ? "tc-receive-up" : ""}`}>
          <span className="tc-recv-label">{copy.youReceive}</span>
          <b className="tc-recv-value">{amt > 0 ? `≈ ${fmtUsd(apiValue)}` : "—"}</b>
          <span className="tc-recv-sub">{amt > 0 ? `${copy.inClaudeApi} · ${isB2c ? interpolate(copy.atTier, { tier: tierName(copy, unlockedTier.code), discount: unlockedDiscount }) : copy.officialPricing}` : copy.enterAmount}</span>
          <div className="tc-recv-meta"><span className="tc-badge">−{unlockedDiscount}%</span><span className="tc-badge tc-badge-soft">{fmtMult(unlockedFrac)} {copy.valueMultiplier}</span></div>
        </div>
      </div>
      {isUpgrade && <p className="tc-upgrade"><span className="tc-upgrade-ic" aria-hidden="true">ⓘ</span>{interpolate(copy.topupUnlocks, { tier: tierName(copy, unlockedTier.code), discount: unlockedDiscount })}</p>}
      <p className="tc-explain">{interpolate(copy.perDollar, { mult: `$${(1 / unlockedFrac).toFixed(2)}` })}</p>
      {amt > 0 && upsell && <p className="tc-nudge">↑ {interpolate(copy.topupUpsell, { amount: fmtUsd(Number(upsell.platformSpendUsd)), tier: tierName(copy, upsell.code), discount: upsell.discountPercent })}</p>}
      <div className="tc-actions"><button className="btn btn-primary" disabled={busy} onClick={start}>{busy ? copy.creating : copy.continuePayment}</button></div>
      {error && <div className="auth-msg err">{error}</div>}{checkout && !checkout.checkoutUrl && <div className="banner">{interpolate(copy.checkoutPending, { id: checkout.id, status: checkout.status })}</div>}
    </div>

    <PricingBanner account={account} />

    <section className="dsec"><div className="dsec-head"><h2>{copy.topupHistory}</h2></div>
      <div className="table-scroll"><table className="mtable">
        <thead><tr><th>{copy.date}</th><th className="tnum">{copy.histPaid}</th><th>{copy.histDiscount}</th><th className="tnum">{copy.histApiValue}</th><th>{copy.reference}</th></tr></thead>
        <tbody>{topups.length === 0 ? <tr><td colSpan={5} className="empty-cell">{copy.noTopups}</td></tr> : topups.map((entry) => {
          const d = (entry as { discountPercent?: number }).discountPercent ?? discountOf(account);
          const paid = Number(entry.amountUsd) || nanoNum(entry.amountNano);
          return <tr key={entry.id}>
            <td>{formatLedgerTime(entry.timestamp, language)}</td>
            <td className="tnum">{normalizeUsd(entry.amountUsd)}</td>
            <td><span className="pill pill-soft">−{d}%</span></td>
            <td className="tnum">≈ {fmtUsd(paid / ((100 - d) / 100))}</td>
            <td>{entry.reference ?? "—"}</td>
          </tr>;
        })}</tbody>
      </table></div>
    </section>
  </section>;
}

const USAGE_WINDOW_DAYS = 30;
const DAY_MS = 86_400_000;

function Usage({ account, ledger, usage }: { account: AccountView; ledger: LedgerEntry[]; usage: UsageView | null }) {
  const copy = useDashboardCopy();
  const { language } = useI18n();
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const models = usage?.models ?? [];
  const modelOfficialTotal = models.reduce((sum, model) => sum + Number(BigInt(model.officialNano)), 0);

  // Скидка определяет, сколько реального Claude API стоит каждый списанный доллар:
  // клиент платит долю frac от официальной цены → официальная ценность = списано / frac.
  const frac = payFraction(account);
  const discount = account.pricing?.discountPercent ?? discountOf(account);
  const netCharged = nanoNum(account.spentNano);
  const officialReceived = netCharged / frac;

  const charges = ledger.filter((entry) => entry.kind === "charge");

  // Дневные корзины официальной ценности за последние 30 дней (реальные данные ledger).
  // "Сегодня" читаем один раз при монтировании — Date.now() нельзя дёргать в чистом рендере.
  const [today] = useState(() => startOfDay(Date.now()));
  const days = Array.from({ length: USAGE_WINDOW_DAYS }, (_, index) => today - (USAGE_WINDOW_DAYS - 1 - index) * DAY_MS);
  const perDay = new Map<number, number>();
  for (const charge of charges) {
    const bucket = startOfDay(ledgerMs(charge.timestamp));
    if (bucket < days[0]!) continue;
    perDay.set(bucket, (perDay.get(bucket) ?? 0) + nanoNum(charge.amountNano) / frac);
  }
  const series = days.map((day) => ({ day, value: perDay.get(day) ?? 0 }));
  const maxValue = Math.max(0, ...series.map((point) => point.value));
  const activeDays = series.filter((point) => point.value > 0).length;
  const windowOfficial = series.reduce((sum, point) => sum + point.value, 0);
  const windowCharges = charges.filter((charge) => startOfDay(ledgerMs(charge.timestamp)) >= days[0]!).length;
  const peak = series.reduce((best, point) => (point.value > best.value ? point : best), { day: today, value: 0 });
  const axisMarks = [0, 7, 14, 21, USAGE_WINDOW_DAYS - 1];

  // Разбивка списаний по API-ключу — наш аналог per-endpoint из референса.
  const keyMap = new Map<string, { key: string; count: number; net: number }>();
  for (const charge of charges) {
    const key = charge.keyMasked ?? "__system__";
    const row = keyMap.get(key) ?? { key, count: 0, net: 0 };
    row.count += 1;
    row.net += nanoNum(charge.amountNano);
    keyMap.set(key, row);
  }
  const keyRows = [...keyMap.values()].sort((a, b) => b.net - a.net);

  return <section className="panel"><PageHeading eyebrow={copy.usageEyebrow} title={copy.usageTitle} subtitle={copy.usageSubtitle} />
    <div className="banner">💡 <b>{copy.sessionSavingTitle}</b><span> {copy.sessionSavingText}</span></div>

    <div className="ov-stats bill4">
      <div className="ovstat"><span className="dlabel">{copy.claudeApiReceived}</span><b className="num accent">{fmtUsd(officialReceived)}</b><span className="dtrend">{copy.atOfficialPrices}</span></div>
      <Stat label={copy.balanceCharged} value={nanoToUsd(account.spentNano)} detail={copy.afterDiscount} />
      <div className="ovstat"><span className="dlabel">{copy.activeDiscount}</span><b className="num">{discount}%</b><span className="dtrend">{fmtMult(frac)} {copy.valueMultiplier}</span></div>
      <div className="ovstat"><span className="dlabel">{copy.availableBalance}</span><b className="num">{normalizeUsd(account.balanceUsd)}</b><span className="dtrend">{nanoNum(account.balanceNano) > 0 ? interpolate(copy.valueOfBalance, { value: fmtUsd(nanoNum(account.balanceNano) / frac) }) : copy.available}</span></div>
    </div>

    <div className="usage-graph">
      <div className="uchart">
        <div className="uchart-head"><b>{copy.usageOverTime}</b><span>{copy.last30Official}</span></div>
        {maxValue === 0 ? <div className="uchart-empty">{copy.noChargesPeriod}</div> : <>
          <div className="uchart-plot">
            {series.map((point) => <div key={point.day} className="uchart-col" title={`${fmtDay(point.day, locale)} · ${fmtUsd(point.value)}`}>
              <div className="uchart-bar" style={{ height: `${maxValue > 0 ? Math.max(point.value > 0 ? 3 : 0, (point.value / maxValue) * 100) : 0}%` }} />
            </div>)}
          </div>
          <div className="uchart-axis">{axisMarks.map((mark) => <span key={mark}>{fmtDay(days[Math.min(mark, days.length - 1)]!, locale)}</span>)}</div>
        </>}
      </div>
      <div className="usum">
        <span className="usum-t">{copy.periodSummary}</span>
        <div className="usum-row"><span>{copy.officialSpend}</span><b className="accent">{fmtUsd(windowOfficial)}</b></div>
        <div className="usum-row"><span>{copy.chargeEvents}</span><b>{windowCharges}</b></div>
        <div className="usum-row"><span>{copy.peakDay}</span><b>{peak.value > 0 ? `${fmtDay(peak.day, locale)} · ${fmtUsd(peak.value)}` : "—"}</b></div>
        <div className="usum-row"><span>{copy.dailyAverage}</span><b>{activeDays > 0 ? fmtUsd(windowOfficial / activeDays) : "—"}</b></div>
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
        <div className="mdist" role="img" aria-label={copy.tokensAndModels}>
          {models.map((model, index) => <div key={model.model} className="mdist-seg" style={{ width: `${modelOfficialTotal > 0 ? Number(BigInt(model.officialNano)) / modelOfficialTotal * 100 : 100 / models.length}%`, background: MODEL_COLORS[index % MODEL_COLORS.length] }} title={`${modelLabel(model.model)} · ${fmtNanoUsd(model.officialNano)}`} />)}
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
        <div><span className="dlabel">{copy.officialValueCol}</span><b>{fmtUsd(officialReceived)}</b></div>
        <div><span className="dlabel">{copy.chargedCol}</span><b>{nanoToUsd(account.spentNano)}</b></div>
      </div>
      <div className="table-scroll"><table className="mtable"><thead><tr><th>{copy.apiKey}</th><th className="tnum">{copy.requests}</th><th className="tnum">{copy.discount}</th><th className="tnum">{copy.valueColumn}</th><th className="tnum">{copy.officialValueCol}</th><th className="tnum">{copy.chargedCol}</th></tr></thead>
        <tbody>{keyRows.length === 0 ? <tr><td colSpan={6} className="empty-cell">{copy.noChargesPeriod}</td></tr> : keyRows.map((row) => <tr key={row.key}>
          <td><code>{row.key === "__system__" ? copy.systemCharge : row.key}</code></td>
          <td className="tnum">{row.count}</td>
          <td className="tnum">{discount}%</td>
          <td className="tnum"><span className="ubadge">{fmtMult(frac)}</span></td>
          <td className="tnum">{fmtUsd(row.net / frac)}</td>
          <td className="tnum mprice">{fmtUsd(row.net)}</td>
        </tr>)}</tbody></table></div>
    </section>

    <section className="dsec"><h2>{copy.transactions}</h2><div className="table-scroll"><table className="mtable"><thead><tr><th>{copy.time}</th><th>{copy.type}</th><th>{copy.apiKey}</th><th>{copy.reference}</th><th className="tnum">{copy.amount}</th></tr></thead><tbody>{ledger.length === 0 ? <tr><td colSpan={5} className="empty-cell">{copy.noLedger}</td></tr> : ledger.map((entry) => <tr key={entry.id}><td>{formatLedgerTime(entry.timestamp, language)}</td><td><span className="pill pill-soft">{entry.kind}</span></td><td><code>{entry.keyMasked ?? "—"}</code></td><td>{entry.reference ?? "—"}</td><td className="tnum">{normalizeUsd(entry.amountUsd)}</td></tr>)}</tbody></table></div></section>
  </section>;
}

function startOfDay(ms: number): number { const date = new Date(ms); date.setHours(0, 0, 0, 0); return date.getTime(); }
function ledgerMs(timestamp: string): number { const numeric = Number(timestamp); return numeric < 10_000_000_000 ? numeric * 1_000 : numeric; }
function fmtDay(ms: number, locale: string): string { return new Date(ms).toLocaleDateString(locale, { month: "numeric", day: "numeric" }); }

// Палитра сегментов по моделям — средние тона, читаются и на светлой, и на тёмной теме.
const MODEL_COLORS = ["#3767f0", "#7c5cff", "#12a594", "#e0913a", "#d6455d", "#8b8f9a"];
function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toLocaleString("en-US", { maximumFractionDigits: 2 })}M`;
  if (n >= 1_000) return `${(n / 1_000).toLocaleString("en-US", { maximumFractionDigits: 1 })}K`;
  return n.toLocaleString("en-US");
}
function fmtNanoUsd(nano: string): string {
  const usd = Number(BigInt(nano)) / 1e9;
  if (usd > 0 && usd < 0.01) return "<$0.01";
  return `$${usd.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;
}
function modelLabel(id: string): string {
  const base = id.replace(/^claude-/i, "").replace(/-\d{8}$/, "");
  const words: string[] = []; const nums: string[] = [];
  for (const part of base.split("-")) { if (/^\d+$/.test(part)) nums.push(part); else if (part) words.push(part[0]!.toUpperCase() + part.slice(1)); }
  return `Claude ${words.join(" ")}${nums.length ? ` ${nums.join(".")}` : ""}`.trim();
}

function Profile({ user, open, onUpdated }: { user: AuthUser; open(section: Section): void; onUpdated(user: AuthUser): void }) {
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
      onUpdated(result.user); setDisplayName(result.user.displayName); setSaved(true);
      window.setTimeout(() => setSaved(false), 2_000);
    } catch (cause) {
      setSaveError(cause instanceof Error ? cause.message : copy.profileSaveError);
    } finally { setSaving(false); }
  }
  return <section className="panel"><PageHeading eyebrow={copy.navAccount} title={copy.profileTitle} subtitle={copy.profileSubtitle} /><div className="prof-grid"><form className="card" onSubmit={saveProfile}><h2>{copy.profileTitle}</h2><div className="set-row"><label className="set-l" htmlFor="profile-email">{copy.email}</label><input id="profile-email" className="set-in" value={user.email} disabled readOnly /></div><div className="set-row"><label className="set-l" htmlFor="profile-display-name">{copy.displayName}</label><input id="profile-display-name" className="set-in" value={displayName} maxLength={80} autoComplete="name" onChange={(event) => { setDisplayName(event.target.value); setSaved(false); setSaveError(null); }} /></div><div className="set-row profile-id-row"><span className="set-l">{copy.userId}</span><span className="uid-wrap"><input className="set-in" value={user.id} aria-label={copy.userId} disabled readOnly /><CopyButton value={user.id} className="uid-copy-button" /></span></div><p className="p-sub">{copy.supportId}</p><div className="profile-meta"><span className="pill">{user.customerType.toUpperCase()}</span><span className="pill pill-soft">Email {user.emailVerified ? copy.verified : copy.pending}</span></div><div className="prof-save"><button className="btn btn-primary btn-sm" type="submit" disabled={saving || unchanged || trimmedName.length === 0}>{saving ? copy.saving : copy.save}</button>{saved && <span className="set-saved always-visible profile-save-success" role="status">{copy.profileSaved}</span>}{saveError && <span className="profile-save-error" role="alert">{saveError}</span>}</div></form>
    <div className="prof-side"><div className="card"><div className="tg-head"><b>{copy.telegram}</b><span className="pill pill-soft">{copy.notConnected}</span></div><p className="p-sub">{copy.telegramHelp}</p><button className="btn btn-primary btn-sm" disabled>{copy.connectTelegram}</button><div className="set-row compact-top"><span className="set-l">{copy.botLanguage}</span><select className="set-in" disabled defaultValue="English"><option>{copy.english}</option><option>{copy.russian}</option></select></div><ToggleRow label={copy.lowBalanceAlerts} /><ToggleRow label={copy.paymentNotifications} /><ToggleRow label={copy.weeklyDigest} /><div className="set-row"><span className="set-l">{copy.lowBalanceThreshold}</span><input className="set-in set-in-sm" type="number" value="10" disabled readOnly /></div></div><div className="card"><div className="tg-head"><b>{copy.securityTitle}</b></div><p className="p-sub">{copy.securityHelp}</p><button className="btn btn-ghost btn-sm" onClick={() => open("security")}>{copy.securityTitle} →</button></div></div></div></section>;
}

function Security({ user, onLogout }: { user: AuthUser; onLogout(): Promise<void> }) {
  const copy = useDashboardCopy();
  const browser = typeof navigator === "undefined" ? "Current browser" : navigator.userAgent;
  return <section className="panel"><PageHeading eyebrow={copy.navAccount} title={copy.securityTitle} subtitle={copy.securitySubtitle} /><div className="card"><p className="p-sub no-top-margin">{copy.twoFactorHelp}</p><button className="btn btn-primary btn-sm" disabled>{copy.enable2fa}</button><span className="future-note">{copy.backendRequired}</span></div>{user.passwordEnabled ? <section className="dsec"><h2>{copy.password}</h2><div className="set-card"><div className="set-row"><span className="set-l">{copy.currentPassword}</span><input className="set-in" type="password" disabled /></div><div className="set-row"><span className="set-l">{copy.newPassword}</span><input className="set-in" type="password" disabled /></div><button className="btn btn-primary btn-sm" disabled>{copy.updatePassword}</button><span className="future-note">{copy.passwordPending}</span></div></section> : <section className="dsec"><h2>{copy.oauthAccess}</h2><div className="set-card oauth-access-card"><p className="p-sub no-margin">{copy.oauthAccessText}</p></div></section>}<section className="dsec"><h2>{copy.activeSessions}</h2><div className="set-card"><div className="set-row"><span className="set-l"><b>{copy.thisDevice}</b><br /><span className="p-sub session-agent">{browser}</span></span><span className="obadge">{copy.activeNow}</span></div><button className="btn btn-ghost btn-sm" onClick={onLogout}>{copy.logoutSession}</button></div></section></section>;
}

function PromoPanel() {
  const copy = useDashboardCopy();
  return <section className="panel"><PageHeading eyebrow={copy.navGrowth} title={copy.promoTitle} subtitle={copy.promoSubtitle} /><div className="card ref-linkcard"><div className="ref-row"><input className="set-in" placeholder="CS-XXXX-XXXX-XXXX" disabled /><button className="btn btn-primary btn-sm" disabled>{copy.activate}</button></div><span className="future-note">{copy.promoPending}</span></div><section className="dsec"><h2>{copy.myActivations}</h2><div className="table-scroll"><table className="mtable"><thead><tr><th>{copy.code}</th><th>{copy.reward}</th><th>{copy.date}</th></tr></thead><tbody><tr><td colSpan={3} className="empty-cell">{copy.noPromos}</td></tr></tbody></table></div></section></section>;
}

function ToggleRow({ label }: { label: string }) {
  return <label className="tgl-row disabled-control"><span>{label}</span><span className="tgl" /></label>;
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
function payFraction(account: AccountView): number {
  const bp = account.pricing?.multiplierBp ?? account.markupBasisPoints;
  return bp && bp > 0 ? bp / 10_000 : 0.4;
}
function discountOf(account: AccountView): number {
  return account.pricing?.discountPercent ?? Math.round((1 - payFraction(account)) * 100);
}
function nanoNum(nano: string): number { return Number(BigInt(nano)) / 1e9; }
function fmtUsd(value: number): string {
  return `$${value.toLocaleString("en-US", { maximumFractionDigits: 2 })}`;
}
function fmtMult(fraction: number): string {
  return `×${(1 / fraction).toLocaleString("en-US", { maximumFractionDigits: 2 })}`;
}
function multFromDiscount(discountPercent: number): string {
  return `×${(100 / (100 - discountPercent)).toLocaleString("en-US", { maximumFractionDigits: 2 })}`;
}
// Тир, который разблокирует разовое пополнение amtUsd (по тем же порогам месячного расхода).
function tierIndexForAmount(amtUsd: number): number {
  let idx = 0;
  B2C_PRICING_MILESTONES.forEach((milestone, index) => { if (amtUsd >= Number(milestone.platformSpendUsd)) idx = index; });
  return idx;
}
