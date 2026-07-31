import Link from "next/link";
import { B2C_DISCOUNT_PERCENT, B2C_VALUE_MULTIPLIER, officialUsageForTopup } from "@/lib/pricing-tiers";
import { T } from "./translated";
import { TopUpAmountInput } from "./topup-amount-input";

// Примеры конверсии по плоской модели: пополнение × 2 = официальное использование API.
const exampleTopups = [50, 100, 1000] as const;

function usd(value: number): string {
  return `$${value.toLocaleString("en-US")}`;
}

export function PricingOverview() {
  return <div className="pricing-overview">
    <div className="pricing-intro" data-reveal-stagger>
      <div className="topup-card">
        <T k="topup_tag" as="span" className="tag">Flexible top-up</T>
        <T k="topup_h" as="h3">Choose any whole USD amount</T>
        <TopUpAmountInput className="topup-preview" initialAmount="1000" showReceive />
        <T k="topup_p" as="p">No catalog and no preset amounts. Enter a whole amount and add exactly that much to your balance.</T>
        <Link className="btn btn-primary" href="/register"><T k="topup_cta">Create account</T></Link>
      </div>
      <div className="business-card">
        <T k="b2b_tag" as="span" className="tag">B2B pricing</T>
        <T k="b2b_h" as="h3">Negotiated business pricing</T>
        <div className="business-preview">
          <div className="business-preview-head">
            <div><T k="b2b_rate_label" as="span">Pricing model</T><T k="b2b_rate_value" as="strong">Custom rate</T></div>
            <T k="b2b_preview" as="span" className="business-access">Private access</T>
          </div>
          <div className="business-terms">
            <div><T k="b2b_discount_label" as="span">Discount</T><T k="b2b_discount_value" as="strong">Volume-based</T></div>
            <div><T k="b2b_onboarding_label" as="span">Onboarding</T><T k="b2b_onboarding_value" as="strong">Direct with our team</T></div>
          </div>
        </div>
        <T k="b2b_p" as="p">Business customers receive an operator-set discount under an individual agreement.</T>
        <a className="business-status btn btn-ghost" href="https://t.me/apiTokenSale" target="_blank" rel="noreferrer"><span className="status-dot" aria-hidden="true" /><T k="b2b_status">Request B2B access</T><span className="business-status-arrow" aria-hidden="true">↗</span></a>
      </div>
    </div>
    <div className="tier-section">
      <div className="tier-heading">
        <div className="tier-heading-copy"><T k="b2c_tag" as="span" className="tag">B2C · Flat pricing</T><T k="b2c_h" as="h3">One flat 50% discount on every request</T></div>
        <div className="tier-rule">
          <div className="tier-rule-item"><T k="flat_rule_rate_label" as="span">Every request</T><T k="flat_rule_rate" as="strong">50% off official provider prices</T></div>
          <div className="tier-rule-item"><T k="flat_rule_topup_label" as="span">Every top-up</T><T k="flat_rule_topup" as="strong">Any whole USD amount at the same rate</T></div>
        </div>
      </div>
      <div className="tier-table-wrap">
        <table className="tier-table">
          <thead><tr><T k="flat_topup_col" as="th">Top up</T><T k="discount_col" as="th">Discount</T><T k="flat_receive_col" as="th">Official API value</T></tr></thead>
          <tbody>{exampleTopups.map((topup) => <tr key={topup}><td>{usd(topup)}</td><td><strong>{B2C_DISCOUNT_PERCENT}%</strong> <em className="tier-mult">×{B2C_VALUE_MULTIPLIER}</em></td><td>{usd(officialUsageForTopup(topup))}</td></tr>)}</tbody>
        </table>
      </div>
      <div className="tier-cards">
        {exampleTopups.map((topup) => <div className="tier-mobile" key={topup}>
          <div className="tier-mobile-head"><strong>{usd(topup)}</strong><b>{B2C_DISCOUNT_PERCENT}% <em className="tier-mult">×{B2C_VALUE_MULTIPLIER}</em></b></div>
          <div className="tier-mobile-row"><T k="discount_col">Discount</T><span>−{B2C_DISCOUNT_PERCENT}%</span></div>
          <div className="tier-mobile-row"><T k="flat_receive_col">Official API value</T><span>{usd(officialUsageForTopup(topup))}</span></div>
        </div>)}
      </div>
      <T k="flat_footnote" as="p" className="tier-footnote">Official API value = top-up ÷ the share paid after the 50% discount. Values are rounded only for display; billing remains exact.</T>
      <Link className="tier-docs-link" href="/tools/claude-api-cost-calculator"><T k="flat_calc_link">Estimate your cost in the free calculator</T> →</Link>
    </div>
  </div>;
}
