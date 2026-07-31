"use client";

import Link from "next/link";
import { useState } from "react";
import { api, type AccountView, type CheckoutView, type LedgerEntry } from "@/lib/api";
import { normalizeUsd } from "@/lib/money";
import { FLAT_DISCOUNT_PERCENT } from "@/lib/pricing-tiers";
import { useI18n } from "@/components/i18n-provider";
import { DOCS_URL } from "@/lib/site-links";
import { checkoutAmountBucket, trackFirstProductEvent, trackProductEvent } from "@/lib/product-analytics";
import {
  NANO_PER_USD, BASIS_POINTS, PageHeading, Stat,
  bpFromDiscount, discountOf, formatFixedRatio, formatLedgerTime, formatMultiplier, formatNanoUsd,
  interpolate, localDashboardCopy, officialNanoFromCharged, useDashboardCopy,
} from "./shared";

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

function PricingBanner({ account }: { account: AccountView }) {
  const copy = useDashboardCopy();
  const { language } = useI18n();
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const pricing = account.pricing;
  if (pricing?.customerType === "b2b") return <section className="pricing-banner pricing-banner-business"><div className="pricing-summary"><div><span className="pricing-kicker">{copy.currentPricing}</span><strong>{copy.businessAgreement}</strong></div><div className="pricing-discount"><b>{pricing.discountPercent}%</b><span>{copy.discount}</span><em className="pricing-mult">{multFromDiscount(pricing.discountPercent, locale)} {copy.valueMultiplier}</em></div></div><p>{copy.negotiatedRate}</p></section>;
  // B2C: одна скидка −50%, без порогов и условий удержания.
  return <section className="pricing-banner pricing-banner-business">
    <div className="pricing-summary">
      <div><span className="pricing-kicker">{copy.currentPricing}</span><strong>{copy.flatRate}</strong></div>
      <div className="pricing-discount"><b>{FLAT_DISCOUNT_PERCENT}%</b><span>{copy.discount}</span><em className="pricing-mult">{multFromDiscount(FLAT_DISCOUNT_PERCENT, locale)} {copy.valueMultiplier}</em></div>
    </div>
    <p>{copy.flatExplainer}</p>
    <div className="pricing-howto-row"><p className="pricing-howto-text">{copy.flatEveryModel}</p><Link className="link pricing-howto-link" href={`${DOCS_URL}#pricing`}>{copy.howPricingWorks} →</Link></div>
  </section>;
}

const TOPUP_PRESETS = [100, 250, 500, 1000] as const;
const WHOLE_USD_AMOUNT = /^[1-9]\d*$/;

export function Credits({ account, ledger, ledgerAvailable }: { account: AccountView; ledger: LedgerEntry[]; ledgerAvailable: boolean }) {
  const copy = useDashboardCopy();
  const { language } = useI18n();
  const localCopy = localDashboardCopy[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const [amount, setAmount] = useState("100");
  const [method, setMethod] = useState<number>(PLATEGA_METHODS[0]!.id);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [checkout, setCheckout] = useState<CheckoutView | null>(null);
  const amountValid = WHOLE_USD_AMOUNT.test(amount);
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

  // B2C получает фиксированные −50%; для B2B сохраняется согласованная ставка аккаунта.
  const isBusiness = account.pricing?.customerType === "b2b";
  const discount = discountOf(account);
  const flatPaymentBp = bpFromDiscount(discount);
  const rateName = isBusiness ? copy.businessRate : copy.flatRate;
  const amountNano = amountValid ? BigInt(amount) * NANO_PER_USD : 0n;
  const apiValueNano = officialNanoFromCharged(amountNano, flatPaymentBp);
  const balanceApiNano = officialNanoFromCharged(BigInt(account.balanceNano), flatPaymentBp);
  const apiValueForPreset = (preset: number) => officialNanoFromCharged(BigInt(preset) * NANO_PER_USD, flatPaymentBp);
  const topups = ledger.filter((entry) => entry.kind === "topup");
  const ledgerMayBePartial = ledger.length >= 100;

  return <section className="panel"><PageHeading eyebrow={copy.creditsEyebrow} title={copy.creditsTitle} subtitle={copy.creditsSubtitle} />
    <div className="credits-stack">
      <div className="ov-stats bill3 tc-stats">
        <div className="ovstat"><span className="dlabel">{copy.currentBalance}</span><b className="num">{normalizeUsd(account.balanceUsd)}</b><span className="dtrend">{BigInt(account.balanceNano) > 0n ? interpolate(copy.valueOfBalance, { value: formatNanoUsd(balanceApiNano, locale) }) : copy.available}</span></div>
        <Stat label={copy.used} value={formatNanoUsd(account.spentNano, locale)} detail={copy.balanceAfterDiscount} />
        <div className="ovstat"><span className="dlabel">{copy.currentPricing}</span><b className="num tc-tier-name">{rateName}</b><span className="dtrend">{discount}% {copy.discount} · {formatMultiplier(flatPaymentBp, locale)} {copy.valueMultiplier}</span></div>
      </div>

      <div className="card topup-convert">
        <div className="tc-head"><h2>{copy.anyWholeAmount}</h2><p className="p-sub" id="topup-amount-help">{copy.checkoutHelp}</p></div>
        <div className="tc-body">
          <div className="tc-input">
            <label className="tc-field"><span className="currency-prefix">$</span><input className="set-in" name="topup-amount" autoComplete="off" inputMode="numeric" pattern="[1-9][0-9]*" value={amount} onChange={(event) => { setAmount(event.target.value); setError(null); }} placeholder="100" aria-label={copy.anyWholeAmount} aria-describedby={amountValidation ? "topup-amount-help topup-amount-error" : "topup-amount-help"} aria-invalid={amountValidation ? true : undefined} /></label>
            <div className="tc-presets" role="group" aria-label={copy.quickAmounts}>{TOPUP_PRESETS.map((preset) => <button key={preset} type="button" className={`tc-preset ${amount === String(preset) ? "on" : ""}`} data-topup-preset={preset} aria-pressed={amount === String(preset)} onClick={() => { setAmount(String(preset)); setError(null); }}><b>${preset}</b><span>{formatNanoUsd(apiValueForPreset(preset), locale)}</span></button>)}</div>
          </div>
          <div className="tc-arrow" aria-hidden="true">→</div>
          <div className="tc-receive tc-receive-up">
            <span className="tc-recv-label">{copy.youReceive}</span>
            <b className="tc-recv-value">{amountNano > 0n ? `≈ ${formatNanoUsd(apiValueNano, locale)}` : "—"}</b>
            <span className="tc-recv-sub">{amountNano <= 0n ? copy.enterAmount : `${copy.inClaudeApi} · −${discount}%`}</span>
            <div className="tc-recv-meta"><span className="tc-badge">−{discount}%</span><span className="tc-badge tc-badge-soft">{formatMultiplier(flatPaymentBp, locale)} {copy.valueMultiplier}</span></div>
          </div>
        </div>
        <p className="tc-explain">{interpolate(copy.perDollar, { mult: formatPerDollar(flatPaymentBp, locale) })}</p>
        <div className="tc-pay">
          <span className="tc-pay-label">{localCopy.payWith}</span>
          <div className="tc-methods" role="radiogroup" aria-label={localCopy.payWith}>
            {PLATEGA_METHODS.map((m) => <label key={m.id} className={`pm-card ${method === m.id ? "on" : ""}`}>
              <input type="radio" name="topup-payment-method" className="sr-only" checked={method === m.id} onChange={() => setMethod(m.id)} />
              <span className={`pm-ic${"logo" in m ? " pm-ic-logo" : ""}`} aria-hidden="true">{m.icon}</span>
              <span className="pm-txt"><b>{language === "ru" ? m.ru : m.en}</b><span>{language === "ru" ? m.ruDesc : m.enDesc}</span></span>
            </label>)}
          </div>
        </div>
        <div className="tc-actions"><button className="btn btn-primary" disabled={busy || !amountValid} onClick={start}>{busy ? copy.creating : copy.continuePayment}</button></div>
        {amountValidation && <div className="auth-msg err" id="topup-amount-error">{amountValidation}</div>}
        {error && <div className="auth-msg err">{error}</div>}{checkout && !checkout.checkoutUrl && <div className="banner">{interpolate(copy.checkoutPending, { id: checkout.id, status: checkout.status })}</div>}
      </div>

      <PricingBanner account={account} />

      {ledgerAvailable && ledgerMayBePartial && <div className="banner">{localCopy.partialLedger}</div>}
      {ledgerAvailable && <section className="dsec credits-history"><div className="dsec-head"><h2 id="topup-history-title">{copy.topupHistory}</h2></div>
        <div className="table-scroll"><table className="mtable topup-history-table" aria-labelledby="topup-history-title">
          <thead><tr><th scope="col">{copy.date}</th><th scope="col" className="tnum">{copy.histPaid}</th><th scope="col">{copy.histDiscount}</th><th scope="col" className="tnum">{copy.histApiValue}</th><th scope="col">{copy.reference}</th></tr></thead>
          <tbody>{topups.length === 0 ? <tr><td colSpan={5} className="empty-cell">{copy.noTopups}</td></tr> : topups.map((entry) => {
            const d = discount;
            const paidNano = BigInt(entry.amountNano);
            const officialValueNano = officialNanoFromCharged(paidNano, bpFromDiscount(d));
            return <tr key={entry.id}>
              <td data-label={copy.date}>{formatLedgerTime(entry.timestamp, language)}</td>
              <td className="tnum" data-label={copy.histPaid}>{formatNanoUsd(paidNano, locale)}</td>
              <td data-label={copy.histDiscount}><span className="pill pill-soft">−{d}%</span></td>
              <td className="tnum" data-label={copy.histApiValue}>≈ {formatNanoUsd(officialValueNano, locale)}</td>
              <td data-label={copy.reference}>{entry.reference ?? "—"}</td>
            </tr>;
          })}</tbody>
        </table></div>
      </section>}
    </div>
  </section>;
}

function formatPerDollar(multiplierBp: bigint, locale: string): string {
  return `$${formatFixedRatio(BASIS_POINTS, multiplierBp, 2, locale)}`;
}
function multFromDiscount(discountPercent: number, locale: string): string {
  return formatMultiplier(bpFromDiscount(discountPercent), locale);
}
function safeCheckoutUrl(rawUrl: string, provider: CheckoutView["provider"]): string | null {
  try {
    const parsed = new URL(rawUrl);
    const allowedOrigins = CHECKOUT_ORIGINS[provider];
    if (parsed.protocol !== "https:" || parsed.username || parsed.password || !allowedOrigins?.has(parsed.origin)) return null;
    return parsed.href;
  } catch { return null; }
}
