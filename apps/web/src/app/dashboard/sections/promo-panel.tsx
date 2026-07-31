"use client";

import { useEffect, useState, type FormEvent } from "react";
import { api, ApiError, type LedgerEntry } from "@/lib/api";
import { useI18n } from "@/components/i18n-provider";
import { trackProductEvent } from "@/lib/product-analytics";
import { PageHeading, formatLedgerTime, formatNanoUsd, localDashboardCopy, useDashboardCopy } from "./shared";

export function PromoPanel({ ledger, ledgerAvailable, ledgerMayBePartial }: { ledger: LedgerEntry[]; ledgerAvailable: boolean; ledgerMayBePartial: boolean }) {
  const copy = useDashboardCopy();
  const { language } = useI18n();
  const localCopy = localDashboardCopy[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<{ usd: string; balance?: string } | null>(null);
  const activations = ledger.filter((entry) => entry.kind !== "charge" && entry.reference?.startsWith("promo:"));

  useEffect(() => {
    // Read ?promo from the URL once on mount (same pattern as RefCapture) — no
    // useSearchParams subscription and no Suspense requirement for a one-time prefill.
    const prefill = new URLSearchParams(window.location.search).get("promo");
    // URL state is browser-owned and intentionally hydrated after mount.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    if (prefill && /^[A-Za-z0-9]{4,32}$/.test(prefill)) setCode(prefill.toUpperCase());
  }, []);

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
          <div className="banner banner-accent" role="status">
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
            placeholder={copy.promoPlaceholder}
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
        <p className="promo-hint">{copy.promoHelp}</p>
      </div>
      {ledgerAvailable && ledgerMayBePartial && <div className="banner">{localCopy.partialLedger}</div>}
      {ledgerAvailable && <section className="dsec promo-history">
        <div className="dsec-head"><h2 id="promo-history-title">{copy.myActivations}</h2></div>
        {activations.length === 0 ? <div className="empty-box">{copy.noPromos}</div> :
        <div className="table-scroll" role="region" tabIndex={0} aria-label={`${copy.myActivations}. ${copy.tableScrollHint}`}>
          <table className="mtable" aria-labelledby="promo-history-title">
            <thead><tr><th>{copy.date}</th><th>{copy.code}</th><th className="tnum">{copy.reward}</th></tr></thead>
            <tbody>{activations.map((entry) => {
              const referenceId = entry.reference?.slice("promo:".length) ?? "";
              return <tr key={entry.id}>
                <td data-label={copy.date}>{formatLedgerTime(entry.timestamp, language)}</td>
                <td data-label={copy.code}><span className="promo-ledger-label" title={entry.reference ?? undefined}>{copy.promoCredit}{referenceId ? ` · …${referenceId.slice(-8)}` : ""}</span></td>
                <td className="tnum" data-label={copy.reward}>+{formatNanoUsd(BigInt(entry.amountNano), locale)}</td>
              </tr>;
            })}</tbody>
          </table>
        </div>}
      </section>}
    </section>
  );
}
