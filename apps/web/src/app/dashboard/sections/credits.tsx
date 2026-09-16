"use client";

import { useState } from "react";
import { api, type AccountView, type CheckoutView, type LedgerEntry } from "@/lib/api";
import { useI18n } from "@/components/i18n-provider";
import { checkoutAmountBucket, trackFirstProductEvent, trackProductEvent } from "@/lib/product-analytics";
import {
  NANO_PER_USD, PageHeading,
  formatLedgerTime, formatNanoUsd, interpolate, localDashboardCopy, useDashboardCopy,
} from "./shared";

const CHECKOUT_ORIGINS: Record<CheckoutView["provider"], ReadonlySet<string>> = {
  cryptomus: new Set(["https://pay.cryptomus.com"]),
  platega: new Set(["https://pay.platega.io", "https://app.platega.io"]),
};
// Top-up amounts are always USD. The payment adapter determines the checkout currency
// for each method and shows the final payment amount before confirmation.
const PLATEGA_METHODS = [
  {
    id: 2, en: "SBP", ru: "СБП",
    enDesc: "Russian banks · instant transfer", ruDesc: "Банки России · мгновенный перевод",
    tag: { en: "Popular", ru: "Популярный" },
    logo: true,
    // Официальный знак СБП (Система быстрых платежей) как значок способа оплаты.
    icon: <svg viewBox="0 0 97.3 120" fill="none"><path d="M0 26.12l14.532 25.975v15.844L.017 93.863z" fill="#5b57a2" /><path d="M55.797 42.643l13.617-8.346 27.868-.026-41.485 25.414z" fill="#d90751" /><path d="M55.72 25.967l.077 34.39-14.566-8.95V0l14.49 25.967z" fill="#fab718" /><path d="M97.282 34.271l-27.869.026-13.693-8.33L41.231 0l56.05 34.271z" fill="#ed6f26" /><path d="M55.797 94.007V77.322l-14.566-8.78.008 51.458z" fill="#63b22f" /><path d="M69.38 85.737L14.531 52.095 0 26.12l97.223 59.583-27.844.034z" fill="#1487c9" /><path d="M41.24 120l14.556-25.993 13.583-8.27 27.843-.034z" fill="#017f36" /><path d="M.017 93.863l41.333-25.32-13.896-8.526-12.922 7.922z" fill="#984995" /></svg>,
  },
  {
    id: 11, en: "Card", ru: "Карта",
    enDesc: "Mir · Russian banks", ruDesc: "Мир · банки России",
    tag: null,
    icon: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round"><rect width="20" height="14" x="2" y="5" rx="2" /><path d="M2 10h20" /><path d="M6 15h4" /></svg>,
  },
  {
    id: 12, en: "International card", ru: "Иностранная карта",
    enDesc: "Visa · Mastercard", ruDesc: "Visa · Mastercard",
    tag: null,
    icon: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round"><rect width="20" height="14" x="2" y="5" rx="2" /><path d="M2 10h20" /><path d="M6 15h4" /><path d="M17 14h1" /></svg>,
  },
  {
    id: 13, en: "Crypto", ru: "Криптовалюта",
    enDesc: "USDT and other coins", ruDesc: "USDT и другие монеты",
    tag: null,
    icon: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round"><circle cx="8" cy="8" r="6" /><path d="M18.09 10.37A6 6 0 1 1 10.34 18" /><path d="M7 6h1v4" /><path d="m16.71 13.88.7.71-2.82 2.82" /></svg>,
  },
] as const;

const pricingCopy = {
  en: {
    offListPrice: "off the official rate",
    addPaid: "Top-up",
    creditAmount: "Amount",
    invalidAmount: "Enter a positive whole amount using digits only.",
    topupTitle: "Amount",
    topupQuestion: "How much will you add?",
    topupHelp: "Whole US dollars only. The payment provider shows the exact amount to pay before you confirm.",
    paymentStep: "Payment method",
    receiptTitle: "Receipt",
    balanceAfter: "Balance after top-up",
    creditedOneToOne: "Top-ups are credited 1:1 in USD. Usage is then billed per model at your discounted rates — how far the balance goes depends on the models you run.",
    lowBalance: "Low balance — top up to keep your API keys working.",
    emptyBalance: "Balance is empty — API requests are paused until you top up.",
    debtBalance: "Balance is negative — settle the debt to resume API requests.",
  },
  ru: {
    offListPrice: "от официального тарифа",
    addPaid: "Пополнение",
    creditAmount: "Сумма",
    invalidAmount: "Введите целую положительную сумму только цифрами.",
    topupTitle: "Сумма пополнения",
    topupQuestion: "Сколько добавить на баланс?",
    topupHelp: "Только целые доллары США. Точную сумму к оплате провайдер покажет перед подтверждением.",
    paymentStep: "Способ оплаты",
    receiptTitle: "Квитанция",
    balanceAfter: "Баланс после пополнения",
    creditedOneToOne: "Пополнение зачисляется 1:1 в долларах. Использование списывается по тарифам конкретной модели с вашей скидкой — на сколько хватит баланса, зависит от моделей, которые вы используете.",
    lowBalance: "Баланс на исходе — пополните, чтобы ключи продолжали работать.",
    emptyBalance: "Баланс пуст — запросы к API приостановлены до пополнения.",
    debtBalance: "Баланс отрицательный — погасите долг, чтобы возобновить запросы.",
  },
} as const;

const TOPUP_PRESETS = [100, 250, 500, 1000] as const;
const WHOLE_USD_AMOUNT = /^[1-9]\d*$/;

export function Credits({ account, ledger, ledgerAvailable }: { account: AccountView; ledger: LedgerEntry[]; ledgerAvailable: boolean }) {
  const copy = useDashboardCopy();
  const { language } = useI18n();
  const localCopy = localDashboardCopy[language];
  const policyCopy = pricingCopy[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const [amount, setAmount] = useState("100");
  const [method, setMethod] = useState<number>(PLATEGA_METHODS[0]!.id);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [checkout, setCheckout] = useState<CheckoutView | null>(null);
  const amountValid = WHOLE_USD_AMOUNT.test(amount);
  const amountValidation = amount === "" || amountValid ? null : policyCopy.invalidAmount;
  const amountUsd = amountValid ? amount : "";
  const discountPercent = account.pricing?.discountPercent ?? null;
  async function start() {
    if (!amountValid || !amountUsd) { setError(policyCopy.invalidAmount); return; }
    setBusy(true); setError(null);
    try {
      const created = await api.createCheckout(amountUsd, method); setCheckout(created);
      const methodName = method === 2 ? "sbp" : method === 11 ? "card" : method === 12 ? "international_card" : method === 13 ? "crypto" : "other";
      trackProductEvent("Checkout Created", { provider: created.provider, payment_method: methodName, amount_bucket: checkoutAmountBucket(amountUsd) });
      trackFirstProductEvent("checkout", "First Checkout Created", { provider: created.provider, payment_method: methodName, amount_bucket: checkoutAmountBucket(amountUsd) });
      if (created.checkoutUrl) {
        const checkoutUrl = safeCheckoutUrl(created.checkoutUrl, created.provider);
        if (!checkoutUrl) { setError(localCopy.invalidCheckoutUrl); return; }
        window.location.assign(checkoutUrl);
      }
    } catch (cause) { setError(cause instanceof Error ? cause.message : copy.createCheckoutError); }
    finally { setBusy(false); }
  }

  const amountNano = amountUsd ? BigInt(amountUsd) * NANO_PER_USD : 0n;
  const balanceNano = BigInt(account.balanceNano);
  const balanceTone = balanceNano < 0n ? "debt" : balanceNano === 0n ? "empty" : balanceNano < 50n * NANO_PER_USD ? "low" : "ok";
  const balanceHint = balanceTone === "debt" ? policyCopy.debtBalance
    : balanceTone === "empty" ? policyCopy.emptyBalance
    : balanceTone === "low" ? policyCopy.lowBalance : null;
  const selectedMethod = PLATEGA_METHODS.find((m) => m.id === method) ?? PLATEGA_METHODS[0]!;
  const selectedMethodName = language === "ru" ? selectedMethod.ru : selectedMethod.en;
  const topups = ledger.filter((entry) => entry.kind === "topup");
  const ledgerMayBePartial = ledger.length >= 100;
  return <section className="panel"><PageHeading eyebrow={copy.creditsEyebrow} title={copy.creditsTitle} subtitle={copy.creditsSubtitle} />
    <div className="credits-layout">
      <section className={`credits-shell tone-${balanceTone}`} aria-label={copy.creditsTitle}>
        <div className="credits-shell-head">
          <span className="credits-shell-label">{policyCopy.paymentStep}</span>
          {discountPercent !== null && <span className="credits-shell-chip">{discountPercent}% {policyCopy.offListPrice}</span>}
        </div>

        <div className="credits-shell-body">
          <div className="credits-form">
            <h2 className="credits-form-title">{policyCopy.topupQuestion}</h2>
            <p className="credits-form-sub" id="topup-amount-help">{policyCopy.topupHelp}</p>

            <div className="credits-amount-row">
              <label className="credits-amount"><span className="credits-amount-cur currency-prefix">$</span><input name="topup-amount" autoComplete="off" inputMode="numeric" pattern="[1-9][0-9]*" value={amount} onChange={(event) => { setAmount(event.target.value); setError(null); }} placeholder="100" aria-label={policyCopy.topupTitle} aria-describedby={amountValidation ? "topup-amount-help topup-amount-error" : "topup-amount-help"} aria-invalid={amountValidation ? true : undefined} /></label>
              <div className="tc-presets" role="group" aria-label={copy.quickAmounts}>{TOPUP_PRESETS.map((preset) => <button key={preset} type="button" className={`tc-preset ${amount === String(preset) ? "on" : ""}`} data-topup-preset={preset} aria-pressed={amount === String(preset)} onClick={() => { setAmount(String(preset)); setError(null); }}>${preset}</button>)}</div>
            </div>
            {amountValidation && <div className="auth-msg err" id="topup-amount-error">{amountValidation}</div>}

            <div className="tc-methods" role="radiogroup" aria-label={policyCopy.paymentStep}>
              {PLATEGA_METHODS.map((m) => <label key={m.id} className={`pm-card ${method === m.id ? "on" : ""}`}>
                <input type="radio" name="topup-payment-method" className="sr-only" checked={method === m.id} onChange={() => setMethod(m.id)} />
                <span className={`pm-ic${"logo" in m ? " pm-ic-logo" : ""}`} aria-hidden="true">{m.icon}</span>
                <span className="pm-txt"><b>{language === "ru" ? m.ru : m.en}</b><span>{language === "ru" ? m.ruDesc : m.enDesc}</span></span>
                {m.tag && <span className="pm-tag">{language === "ru" ? m.tag.ru : m.tag.en}</span>}
              </label>)}
            </div>

            <div className="credits-shell-balance">
              <span className="credits-shell-balance-label">{copy.currentBalance}</span>
              <strong className={balanceNano < 0n ? "is-negative" : undefined}>{formatNanoUsd(account.balanceNano, locale)}</strong>
              {balanceHint && <p className="credits-balance-hint">{balanceHint}</p>}
            </div>
          </div>

          <aside className="credits-receipt" aria-label={policyCopy.receiptTitle}>
            <div className="credits-receipt-head"><span>{policyCopy.receiptTitle}</span>{discountPercent !== null && <span>{discountPercent}% {policyCopy.offListPrice}</span>}</div>
            <dl className="credits-receipt-lines">
              <div><dt>{copy.currentBalance}</dt><dd className={balanceNano < 0n ? "is-negative" : undefined}>{formatNanoUsd(account.balanceNano, locale)}</dd></div>
              <div><dt>{policyCopy.addPaid}</dt><dd>{amountNano > 0n ? `+${formatNanoUsd(amountNano, locale)}` : "—"}</dd></div>
              <div><dt>{copy.currentPricing}</dt><dd>{discountPercent === null ? "—" : `${discountPercent}%`}</dd></div>
              <div><dt>{policyCopy.paymentStep}</dt><dd>{selectedMethodName}</dd></div>
            </dl>
            <div className="credits-receipt-total">
              <span>{policyCopy.balanceAfter}</span>
              <strong>{amountNano > 0n ? formatNanoUsd(balanceNano + amountNano, locale) : "—"}</strong>
              <small>{policyCopy.creditedOneToOne}</small>
            </div>
            {/* .topup-simple-footer is kept as the stable checkout hook (tests, legacy CSS). */}
            <div className="topup-simple-footer">
              <div><span>{policyCopy.addPaid}</span><strong>{amountNano > 0n ? formatNanoUsd(amountNano, locale) : "—"}</strong></div>
              <button className="btn credits-receipt-cta" disabled={busy || !amountValid} onClick={start}>{busy ? copy.creating : copy.continuePayment}</button>
            </div>
          </aside>
        </div>
        {error && <div className="auth-msg err credits-shell-msg">{error}</div>}
        {checkout && !checkout.checkoutUrl && <div className="banner credits-shell-msg">{interpolate(copy.checkoutPending, { id: checkout.id, status: checkout.status })}</div>}
      </section>

      {ledgerAvailable && ledgerMayBePartial && <div className="banner credits-history-banner">{localCopy.partialLedger}</div>}
      {ledgerAvailable && <section className="dsec credits-history"><div className="dsec-head"><h2 id="topup-history-title">{copy.topupHistory}</h2></div>
        <div className="table-scroll"><table className="mtable topup-history-table" aria-labelledby="topup-history-title">
          <thead><tr><th scope="col">{copy.date}</th><th scope="col" className="tnum">{policyCopy.creditAmount}</th><th scope="col">{copy.reference}</th></tr></thead>
          <tbody>{topups.length === 0 ? <tr><td colSpan={3} className="empty-cell">{copy.noTopups}</td></tr> : topups.map((entry) => {
            return <tr key={entry.id}>
              <td data-label={copy.date}><span className="topup-history-value">{formatLedgerTime(entry.timestamp, language)}</span></td>
              <td className="tnum" data-label={policyCopy.creditAmount}><span className="topup-history-value">{formatNanoUsd(entry.amountNano, locale)}</span></td>
              <td data-label={copy.reference}><span className="topup-history-value">{entry.reference ?? "—"}</span></td>
            </tr>;
          })}</tbody>
        </table></div>
      </section>}
    </div>
  </section>;
}

function safeCheckoutUrl(rawUrl: string, provider: CheckoutView["provider"]): string | null {
  try {
    const parsed = new URL(rawUrl);
    const allowedOrigins = CHECKOUT_ORIGINS[provider];
    if (parsed.protocol !== "https:" || parsed.username || parsed.password || !allowedOrigins?.has(parsed.origin)) return null;
    return parsed.href;
  } catch { return null; }
}
