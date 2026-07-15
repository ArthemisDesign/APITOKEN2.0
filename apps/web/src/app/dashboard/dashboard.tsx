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
import { dashboardHref, parseDashboardSection, type DashboardSection } from "./dashboard-route";

type Section = DashboardSection;

const navigation: Array<{ section?: Section; label: string; icon: string; href?: string; group?: string }> = [
  { group: "Start", section: "overview", label: "Overview", icon: "▦" },
  { section: "keys", label: "API keys", icon: "⚿" },
  { section: "credits", label: "Top up balance", icon: "＋" },
  { group: "Growth", section: "refer", label: "Refer & earn", icon: "◈" },
  { section: "promos", label: "Promo codes", icon: "%" },
  { group: "Activity", section: "usage", label: "Usage", icon: "◔" },
  { section: "orders", label: "Orders", icon: "▣" },
  { group: "Account", section: "profile", label: "Profile", icon: "◍" },
  { section: "security", label: "Security", icon: "⛨" },
  { href: "/docs", label: "Docs", icon: "❯" },
];

export function Dashboard() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const { language, setLanguage } = useI18n();
  const section = parseDashboardSection(searchParams.get("view"));
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
      setError(cause instanceof Error ? cause.message : "Unable to load your account");
    } finally { setLoading(false); }
  }, [router]);

  useEffect(() => {
    document.body.classList.add("app-body");
    const timer = window.setTimeout(() => { void load(); }, 0);
    return () => { window.clearTimeout(timer); document.body.classList.remove("app-body"); };
  }, [load]);

  async function logout() {
    await api.logout().catch(() => undefined); router.replace("/login"); router.refresh();
  }

  function open(next: Section) {
    setSideOpen(false);
    router.push(dashboardHref(next), { scroll: false });
    window.scrollTo({ top: 0, behavior: "auto" });
  }

  if (loading) return <div className="dashboard-loading"><span className="brand">apiToken.sale</span><p>Loading your account…</p></div>;
  if (!user || !account) return <div className="wrap guard"><div className="auth-card"><p>{error ?? "Please log in to open your dashboard."}</p><Link className="btn btn-primary" href="/login">Log in</Link></div></div>;

  const activeKeys = keys.filter((key) => key.status === "active");
  return <div className="app">
    <aside className={`side ${sideOpen ? "open" : ""}`}>
      <Link className="brand side-brand" href="/"><BrandImages />apiToken.sale</Link>
      <nav className="side-nav">
        {navigation.map((item, index) => <div key={`${item.label}-${index}`} className="side-nav-item">
          {item.group && <span className="side-group">{item.group}</span>}
          {item.href ? <Link className="side-link" href={item.href}><span className="si">{item.icon}</span><span>{item.label}</span></Link> :
            <button className={section === item.section ? "on" : ""} aria-current={section === item.section ? "page" : undefined} onClick={() => open(item.section!)}><span className="si">{item.icon}</span><span>{item.label}</span></button>}
        </div>)}
      </nav>
      <div className="side-foot">
        <div className="side-tools"><div className="lang"><button className={language === "en" ? "active" : ""} onClick={() => setLanguage("en")}>EN</button><button className={language === "ru" ? "active" : ""} onClick={() => setLanguage("ru")}>RU</button></div><ThemeToggle /></div>
        <div className="side-user"><span className="side-av">{user.email[0]?.toUpperCase()}</span><div className="side-uinfo"><b>{user.email.split("@")[0]}</b><span>{user.email}</span></div></div>
        <button className="btn btn-ghost btn-sm side-logout" onClick={logout}>Log out</button>
      </div>
    </aside>
    <button className={`side-scrim ${sideOpen ? "show" : ""}`} onClick={() => setSideOpen(false)} aria-label="Close menu" />
    <main className="app-main">
      <header className="app-top"><button className="app-burger" onClick={() => setSideOpen(true)} aria-label="Menu">☰</button><div className="app-top-h"><div className="app-title">{navigation.find((item) => item.section === section)?.label}</div><span className="app-top-email">{user.email}</span></div><button className="btn btn-primary btn-sm" onClick={() => open("credits")}>Top up</button></header>
      <div className="app-body-in">
        {error && <div className="banner banner-error">{error} <button className="btn btn-ghost btn-sm" onClick={load}>Retry</button></div>}
        {section === "overview" && <Overview account={account} keys={activeKeys} ledger={ledger} open={open} />}
        {section === "keys" && <ApiKeys keys={keys} onChanged={load} />}
        {section === "credits" && <Credits account={account} />}
        {section === "usage" && <Usage account={account} ledger={ledger} open={open} />}
        {section === "profile" && <Profile user={user} open={open} />}
        {section === "security" && <Security onLogout={logout} />}
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
  const spent = nanoToUsd(account.spentNano);
  const total = nanoToUsd((BigInt(account.balanceNano) + BigInt(account.spentNano)).toString());
  const charged = ledger.filter((entry) => entry.kind === "charge").length;
  const complete = 1 + Number(keys.length > 0) + Number(BigInt(account.spentNano) > 0n);
  return <section className="panel">
    <PageHeading eyebrow="Workspace" title="Overview" subtitle="Your live account state at a glance." />
    <PricingBanner account={account} />
    <div className="ov-lead"><div className="ov-lead-l card"><span className="chip">Next step</span><h2>{keys.length ? "Connect your API key" : "Create an API key"}</h2><p>One key works with every model supported by the core gateway.</p><button className="btn btn-primary btn-sm" onClick={() => open("keys")}>⚿ {keys.length ? "Manage API keys" : "Get an API key"}</button></div>
      <div className="ov-lead-r card"><div className="rd-head"><div><b>Account readiness</b><span className="rd-steps">{complete}/3 steps complete</span></div><span className="rd-pct">{Math.round(complete / 3 * 100)}%</span></div><div className="rd-bar"><div className="rd-bar-fill" style={{ width: `${complete / 3 * 100}%` }} /></div><Readiness label="Account active" done /><Readiness label="API key" done={keys.length > 0} /><Readiness label="First charged request" done={BigInt(account.spentNano) > 0n} /></div></div>
    <div className="ov-stats"><Stat label="Remaining balance" value={normalizeUsd(account.balanceUsd)} detail={`of ${total} funded`} /><Stat label="Used" value={spent} detail="Balance charged after discount" /><Stat label="Active keys" value={String(keys.length)} detail="Manage →" onClick={() => open("keys")} /><Stat label="Recent charges" value={String(charged)} detail="View ledger →" onClick={() => open("usage")} /></div>
    <div className="ov-tiles"><Tile icon="▤" title="Top up balance" subtitle="Add API balance" onClick={() => open("credits")} /><Tile icon="⚿" title="API keys" subtitle="Create and revoke keys" onClick={() => open("keys")} /><Tile icon="◍" title="Profile & security" subtitle="Account and access" onClick={() => open("profile")} /></div>
    <div className="card connect-card"><div className="cc-head"><div><span className="cc-eyebrow">API · CLAUDE CODE · SDK</span><h2>Connect Claude without digging through docs</h2><p>Use the Anthropic-compatible endpoint with your one-time API key.</p></div><button className="btn btn-primary btn-sm" onClick={() => open("keys")}>Get an API key</button></div><div className="cc-ep"><span className="cc-ep-l">Anthropic endpoint</span><div className="ep-row"><code>https://api.apitoken.sale</code><CopyButton value="https://api.apitoken.sale" /></div></div></div>
  </section>;
}

function Readiness({ label, done }: { label: string; done: boolean }) { return <div className="rd-row"><span>{label}</span><span className={`rd-st ${done ? "done" : "todo"}`}>{done ? "done" : "todo"}</span></div>; }
function Stat({ label, value, detail, onClick }: { label: string; value: string; detail: string; onClick?: () => void }) { return <div className="ovstat"><span className="dlabel">{label}</span><b className="num">{value}</b>{onClick ? <button className="dtrend link plain-button" onClick={onClick}>{detail}</button> : <span className="dtrend">{detail}</span>}</div>; }
function Tile({ icon, title, subtitle, onClick }: { icon: string; title: string; subtitle: string; onClick(): void }) { return <button className="ov-tile" onClick={onClick}><span className="ovt-ic">{icon}</span><span className="ovt-t"><b>{title}</b><span>{subtitle}</span></span><span className="ovt-a">→</span></button>; }

function PricingBanner({ account }: { account: AccountView }) {
  const pricing = account.pricing;
  if (!pricing) return null;
  if (pricing.customerType === "b2b") return <section className="pricing-banner pricing-banner-business"><div className="pricing-summary"><div><span className="pricing-kicker">Current pricing</span><strong>Business agreement</strong></div><div className="pricing-discount"><b>{pricing.discountPercent}%</b><span>discount</span></div></div><p>Your negotiated rate is active across every supported model.</p></section>;
  const currentIndex = Math.max(0, B2C_PRICING_MILESTONES.findIndex((tier) => tier.code === pricing.tier));
  const currentTier = B2C_PRICING_MILESTONES[currentIndex]!;
  const progress = pricingMilestoneProgress(pricing.tier, pricing.spentNano);
  const trackStyle = { "--tier-progress": `${progress}%` } as CSSProperties;
  return <section className="pricing-banner pricing-banner-milestones">
    <div className="pricing-summary">
      <div><span className="pricing-kicker">Monthly tier progress</span><strong>{currentTier.label} tier</strong></div>
      <div className="pricing-discount"><b>{pricing.discountPercent}%</b><span>discount</span></div>
    </div>
    <div className="pricing-milestone-status">
      <div className="pricing-status-item"><span>This month</span><strong>{nanoToUsd(pricing.spentNano)}</strong><small>platform spend</small></div>
      {pricing.nextTier ? <div className="pricing-status-item pricing-status-next"><span>Next milestone</span><strong>Spend {nanoToUsd(pricing.nextTier.remainingNano)} more</strong><small>Unlock {titleCase(pricing.nextTier.tier)} · {pricing.nextTier.discountPercent}% discount</small></div> :
        <div className="pricing-status-item pricing-status-next"><span>Milestones complete</span><strong>Highest tier reached</strong><small>Scale · {pricing.discountPercent}% discount</small></div>}
    </div>
    <div className="pricing-milestone-track" style={trackStyle} aria-label={`${Math.round(progress)}% progress through pricing milestones`}>
      <div className="pricing-track-line" aria-hidden="true"><span /></div>
      <ol className="pricing-milestone-list">
        {B2C_PRICING_MILESTONES.map((tier, index) => {
          const state = index < currentIndex ? "complete" : index === currentIndex ? "current" : "upcoming";
          return <li className={`pricing-milestone ${state}`} key={tier.code}>
            <span className="pricing-milestone-dot" aria-hidden="true">{index < currentIndex ? "✓" : index + 1}</span>
            <div><strong>{tier.label}</strong><span>{tier.discountPercent}% discount</span><small>{index === 0 ? "Starting tier" : `At ${formatWholeUsd(tier.platformSpendUsd)} spend`}</small></div>
          </li>;
        })}
      </ol>
    </div>
  </section>;
}

function ApiKeys({ keys, onChanged }: { keys: ApiKeyView[]; onChanged(): Promise<void> }) {
  const [label, setLabel] = useState("");
  const [issued, setIssued] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  async function create() {
    setBusy(true); setError(null);
    try { const created = await api.createApiKey(label.trim() || undefined); setIssued(created.key ?? null); setLabel(""); await onChanged(); }
    catch (cause) { setError(cause instanceof Error ? cause.message : "Unable to create the key"); }
    finally { setBusy(false); }
  }
  async function revoke(id: string) {
    if (!window.confirm("Revoke this API key? This cannot be undone.")) return;
    setBusy(true); try { await api.revokeApiKey(id); await onChanged(); } catch (cause) { setError(cause instanceof Error ? cause.message : "Unable to revoke the key"); } finally { setBusy(false); }
  }
  const snippet = `curl https://api.apitoken.sale/v1/messages \\\n  -H "x-api-key: YOUR_API_KEY" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{"model":"claude-opus-4-8","max_tokens":1024,"messages":[{"role":"user","content":"Hello"}]}'`;
  return <section className="panel"><PageHeading eyebrow="Start" title="API keys" subtitle="Create a universal key and connect any Anthropic-compatible client." />
    <div className="card ep-card"><span className="cc-ep-l">API endpoint</span><div className="ep-row"><div><span className="ep-t">Anthropic-compatible</span><code>https://api.apitoken.sale</code></div><CopyButton value="https://api.apitoken.sale" /></div></div>
    {issued && <div className="card secret-card"><span className="chip">Shown once</span><h2>Copy your new API key now</h2><p>The commercial backend does not store the raw secret and cannot reveal it again.</p><code>{issued}</code><CopyButton value={issued} /><button className="btn btn-ghost btn-sm" onClick={() => setIssued(null)}>I saved it</button></div>}
    <section className="dsec"><div className="dsec-head"><h2>Universal keys</h2><div className="key-create"><input className="set-in" value={label} onChange={(event) => setLabel(event.target.value)} maxLength={100} placeholder="Optional label" /><button className="btn btn-primary btn-sm" disabled={busy} onClick={create}>＋ New key</button></div></div>{error && <div className="banner banner-error">{error}</div>}
      <div className="keys">{keys.length === 0 ? <div className="empty-box">No API keys yet.</div> : keys.map((key) => <div className="keyrow" key={key.id}><code className="kval">{key.keyMasked}</code><div className="kmeta">{key.label || "Unlabelled key"} · created {new Date(key.createdAt).toLocaleDateString()} · spent {normalizeUsd(key.spentUsd)}</div><div className="kacts"><span className={`pill ${key.status === "active" ? "" : "pill-soft"}`}>{key.status}</span>{key.status === "active" && <button className="btn btn-ghost btn-sm" disabled={busy} onClick={() => revoke(key.id)}>Revoke</button>}</div></div>)}</div>
    </section><section className="dsec"><div className="dsec-head"><h2>Quick start</h2><CopyButton value={snippet} /></div><pre className="code-card"><code>{snippet}</code></pre></section>
  </section>;
}

function Credits({ account }: { account: AccountView }) {
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
    } catch (cause) { setError(cause instanceof Error ? cause.message : "Unable to create checkout"); }
    finally { setBusy(false); }
  }
  return <section className="panel"><PageHeading eyebrow="Start" title="Top up balance" subtitle="Enter the whole USD amount you want to add to your API balance." />
    <div className="ov-stats bill3"><Stat label="Current balance" value={normalizeUsd(account.balanceUsd)} detail="Available" /><Stat label="Used" value={nanoToUsd(account.spentNano)} detail="Balance charged after discount" /><Stat label="Reserved" value={nanoToUsd(account.reservedNano)} detail="In-flight requests" /></div>
    <PricingBanner account={account} />
    <div className="card checkout-card"><div><span className="chip">Crypto checkout</span><h2>Top up any whole USD amount</h2><p className="p-sub">No preset amounts and no decimals. The payment provider will show the final cryptocurrency amount.</p></div><div className="checkout-entry"><span className="currency-prefix">$</span><input className="set-in" inputMode="numeric" pattern="[1-9][0-9]*" value={amount} onChange={(event) => setAmount(event.target.value.replace(/\D/g, ""))} placeholder="100" /><button className="btn btn-primary" disabled={busy} onClick={start}>{busy ? "Creating…" : "Continue to payment"}</button></div>{error && <div className="auth-msg err">{error}</div>}{checkout && !checkout.checkoutUrl && <div className="banner">Checkout {checkout.id} is {checkout.status}. Refresh later to obtain its payment URL.</div>}</div>
  </section>;
}

function Usage({ account, ledger, open }: { account: AccountView; ledger: LedgerEntry[]; open(section: Section): void }) {
  const discount = account.pricing?.discountPercent;
  return <section className="panel"><PageHeading eyebrow="Activity" title="Usage" subtitle="Official-spend billing and authoritative balance movements from the core engine ledger." />
    <div className="banner">💡 <b>Save tokens by working in long sessions.</b><span> Long sessions can take advantage of prompt caching, reducing repeated-context cost.</span></div>
    <div className="us-cards">
      <div className="card us-route"><div className="us-route-h"><b>API routing</b><span className="pill">API</span></div><span className="dlabel">Available balance</span><div className="us-bal">{normalizeUsd(account.balanceUsd)}</div><span className="p-sub no-margin">{nanoToUsd(account.spentNano)} total balance charged after discount</span><p className="p-sub compact-top">The same balance covers all Claude models exposed by the core gateway.</p></div>
      <div className="card us-health"><div className="us-route-h"><b>API health</b><span className="pill pill-soft">Analytics pending</span></div><span className="p-sub no-margin">Request-level health will appear when the backend exposes authoritative metrics.</span><div className="us-health-row"><div><span className="dlabel">Success rate</span><b className="num">—</b></div><div><span className="dlabel">Avg latency</span><b className="num">—</b></div></div><button className="btn btn-primary btn-sm compact-top" onClick={() => open("credits")}>Top up balance</button></div>
    </div>
    <div className="ov-stats bill4"><Stat label="Requests" value="—" detail="Analytics pending" /><Stat label="Official API spend" value="—" detail="Per-request metric pending" /><Stat label="Balance charged" value={nanoToUsd(account.spentNano)} detail="After discount" /><Stat label="Active discount" value={discount === undefined ? "—" : `${discount}%`} detail={account.pricing?.customerType === "b2b" ? "Business rate" : "Current B2C tier"} /></div>
    <section className="dsec"><div className="dsec-head analytics-heading"><div><h2>Usage analytics</h2><p>Charts and model-level breakdowns will appear when the backend exposes authoritative request metrics.</p></div><span className="pill pill-soft">Coming soon</span></div><div className="analytics-placeholder"><div><span className="dlabel">Usage trend</span><b>Official API spend over time</b></div><div><span className="dlabel">Model breakdown</span><b>Spend grouped by Claude model</b></div></div></section>
    <section className="dsec"><h2>Transactions and charges</h2><div className="table-scroll"><table className="mtable"><thead><tr><th>Time</th><th>Type</th><th>API key</th><th>Reference</th><th className="tnum">Amount</th></tr></thead><tbody>{ledger.length === 0 ? <tr><td colSpan={5} className="empty-cell">No ledger activity yet.</td></tr> : ledger.map((entry) => <tr key={entry.id}><td>{formatLedgerTime(entry.timestamp)}</td><td><span className="pill pill-soft">{entry.kind}</span></td><td><code>{entry.keyMasked ?? "—"}</code></td><td>{entry.reference ?? "—"}</td><td className="tnum">{normalizeUsd(entry.amountUsd)}</td></tr>)}</tbody></table></div></section>
  </section>;
}

function Profile({ user, open }: { user: AuthUser; open(section: Section): void }) {
  return <section className="panel"><PageHeading eyebrow="Account" title="Profile" subtitle="Verified account details from the commercial backend." /><div className="prof-grid"><div className="card"><h2>Profile</h2><div className="set-row"><span className="set-l">Email</span><input className="set-in" value={user.email} disabled readOnly /></div><div className="set-row"><span className="set-l">Display name</span><input className="set-in" value={user.email.split("@")[0]} disabled readOnly /></div><div className="set-row"><span className="set-l">User ID</span><span className="uid-wrap"><input className="set-in" value={user.id} readOnly /><CopyButton value={user.id} /></span></div><p className="p-sub">Keep this ID handy when asking support to inspect API usage or billing events.</p><div className="profile-meta"><span className="pill">{user.customerType.toUpperCase()}</span><span className="pill pill-soft">Email {user.emailVerified ? "verified" : "pending"}</span></div><div className="prof-save"><button className="btn btn-primary btn-sm" disabled>Save</button><span className="set-saved always-visible">Profile editing is not connected yet</span></div></div>
    <div className="prof-side"><div className="card"><div className="tg-head"><b>Telegram</b><span className="pill pill-soft">Not connected</span></div><p className="p-sub">Connect Telegram to receive balance and usage updates.</p><button className="btn btn-primary btn-sm" disabled>Connect Telegram</button><div className="set-row compact-top"><span className="set-l">Bot language</span><select className="set-in" disabled defaultValue="English"><option>English</option><option>Русский</option></select></div><ToggleRow label="Low balance alerts" /><ToggleRow label="Payment notifications" /><ToggleRow label="Weekly usage digest" /><div className="set-row"><span className="set-l">Low balance threshold, USD</span><input className="set-in set-in-sm" type="number" value="10" disabled readOnly /></div></div><div className="card"><div className="tg-head"><b>Security</b></div><p className="p-sub">Review account access and the security controls planned for production use.</p><button className="btn btn-ghost btn-sm" onClick={() => open("security")}>Security →</button></div></div></div></section>;
}

function Security({ onLogout }: { onLogout(): Promise<void> }) {
  const browser = typeof navigator === "undefined" ? "Current browser" : navigator.userAgent;
  return <section className="panel"><PageHeading eyebrow="Account" title="Security" subtitle="Your session is stored in a Secure, HttpOnly backend cookie." /><div className="card"><p className="p-sub no-top-margin">Use Google Authenticator, Authy, 1Password or any TOTP client when two-factor authentication becomes available.</p><button className="btn btn-primary btn-sm" disabled>Enable 2FA</button><span className="future-note">Backend support required</span></div><section className="dsec"><h2>Password</h2><div className="set-card"><div className="set-row"><span className="set-l">Current password</span><input className="set-in" type="password" disabled /></div><div className="set-row"><span className="set-l">New password</span><input className="set-in" type="password" disabled /></div><button className="btn btn-primary btn-sm" disabled>Update password</button><span className="future-note">Password changes are not connected yet</span></div></section><section className="dsec"><h2>Active sessions</h2><div className="set-card"><div className="set-row"><span className="set-l"><b>This device</b><br /><span className="p-sub session-agent">{browser}</span></span><span className="obadge">Active now</span></div><button className="btn btn-ghost btn-sm" onClick={onLogout}>Log out this session</button></div></section></section>;
}

function ReferralPanel() {
  return <section className="panel"><PageHeading eyebrow="Growth" title="Referral program" subtitle="The original referral workspace is preserved, but no rewards or links are created until its backend exists." /><div className="card ref-linkcard"><span className="cc-ep-l">Your referral link</span><div className="ref-row"><input className="set-in" value="Available later" disabled readOnly /><button className="btn btn-primary btn-sm" disabled>Copy link</button></div></div><div className="claim-grid"><div className="card claim-card"><div className="claim-top"><b>Claim as API usage</b><span className="pill pill-soft">Pending usage</span></div><div className="claim-amt">$0.00</div><p>Confirmed referral rewards will be claimable to your API balance.</p><button className="btn btn-primary btn-sm" disabled>Claim usage →</button></div><div className="card claim-card"><div className="claim-top"><b>Claim as USDT (Solana)</b><span className="pill pill-soft">Pending USDT</span></div><div className="claim-amt">$0.00</div><p>USDT payouts will require manual review.</p><button className="btn btn-ghost btn-sm" disabled>Claim USDT →</button></div></div><div className="ov-stats bill4"><Stat label="Joined" value="0" detail="Not active" /><Stat label="Paid" value="0" detail="Not active" /><Stat label="Confirmed rewards" value="0" detail="Not active" /><Stat label="Total usage reward" value="$0.00" detail="Not active" /></div><section className="dsec"><h2>Referral rewards log</h2><div className="empty-box">No confirmed rewards yet.</div></section></section>;
}

function PromoPanel() {
  return <section className="panel"><PageHeading eyebrow="Growth" title="Promo codes" subtitle="The redemption interface is preserved and will activate only after a server-side promo domain is implemented." /><div className="card ref-linkcard"><div className="ref-row"><input className="set-in" placeholder="CS-XXXX-XXXX-XXXX" disabled /><button className="btn btn-primary btn-sm" disabled>Activate</button></div><span className="future-note">Promo-code redemption is not active yet.</span></div><section className="dsec"><h2>My activations</h2><div className="table-scroll"><table className="mtable"><thead><tr><th>Code</th><th>Reward</th><th>Date</th></tr></thead><tbody><tr><td colSpan={3} className="empty-cell">No promo-code activations.</td></tr></tbody></table></div></section></section>;
}

function OrdersPanel() {
  return <section className="panel"><PageHeading eyebrow="Activity" title="Orders" subtitle="Balance top-ups and grants will appear here after the backend exposes authenticated checkout history." /><div className="table-scroll"><table className="mtable"><thead><tr><th>Order</th><th>Date</th><th>Description</th><th className="tnum">Amount</th><th>Status</th></tr></thead><tbody><tr><td colSpan={5} className="empty-cell">No order history is available yet.</td></tr></tbody></table></div></section>;
}

function ToggleRow({ label }: { label: string }) {
  return <label className="tgl-row disabled-control"><span>{label}</span><span className="tgl" /></label>;
}

function CopyButton({ value }: { value: string }) {
  const [copied, setCopied] = useState(false);
  async function copy() { await navigator.clipboard.writeText(value); setCopied(true); window.setTimeout(() => setCopied(false), 1_200); }
  return <button className="btn btn-ghost btn-sm" onClick={copy}>{copied ? "Copied" : "Copy"}</button>;
}

function formatLedgerTime(timestamp: string): string {
  const numeric = Number(timestamp);
  const milliseconds = numeric < 10_000_000_000 ? numeric * 1_000 : numeric;
  return new Date(milliseconds).toLocaleString();
}

function titleCase(value: string): string { return value.charAt(0).toUpperCase() + value.slice(1); }
