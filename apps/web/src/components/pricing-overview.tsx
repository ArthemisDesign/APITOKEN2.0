import Link from "next/link";
import { T } from "./translated";
import { TopUpAmountInput } from "./topup-amount-input";

const tiers = [
  ["tier_starter", "60%", "$0", "$0"],
  ["tier_builder", "65%", "$25", "$70"],
  ["tier_pro", "70%", "$75", "$250"],
  ["tier_studio", "75%", "$200", "$800"],
  ["tier_scale", "80%", "$500", "$2,500"],
] as const;

export function PricingOverview() {
  return <div className="pricing-overview">
    <div className="pricing-intro" data-reveal-stagger>
      <div className="topup-card">
        <T k="topup_tag" as="span" className="tag">Flexible top-up</T>
        <T k="topup_h" as="h3">Choose any whole USD amount</T>
        <TopUpAmountInput className="topup-preview" initialAmount="100" />
        <T k="topup_p" as="p">No catalog and no preset amounts. Enter a whole amount and add exactly that much to your balance.</T>
        <Link className="btn btn-primary" href="/register"><T k="topup_cta">Create account</T></Link>
      </div>
      <div className="business-card">
        <T k="b2b_tag" as="span" className="tag">B2B pricing</T>
        <T k="b2b_h" as="h3">Negotiated business pricing</T>
        <div className="business-preview" aria-hidden="true"><strong>B2B</strong><T k="b2b_preview">Private access</T></div>
        <T k="b2b_p" as="p">Business customers receive an operator-set discount under an individual agreement.</T>
        <div className="business-status"><span className="status-dot" aria-hidden="true" /><T k="b2b_status">Invite-only registration</T></div>
      </div>
    </div>
    <div className="tier-section">
      <div className="tier-heading">
        <div className="tier-heading-copy"><T k="b2c_tag" as="span" className="tag">B2C · Progressive pricing</T><T k="b2c_h" as="h3">Your discount grows with monthly usage</T></div>
        <div className="tier-rule">
          <div className="tier-rule-item"><T k="tier_rule_keep_label" as="span">Carry forward</T><T k="tier_rule_keep" as="strong">Keep your achieved tier next month</T></div>
          <div className="tier-rule-item"><T k="tier_rule_miss_label" as="span">If you miss the target</T><T k="tier_rule_miss" as="strong">Move down only one tier</T></div>
        </div>
      </div>
      <div className="tier-table-wrap">
        <table className="tier-table">
          <thead><tr><T k="tier_col" as="th">Tier</T><T k="discount_col" as="th">Discount</T><T k="local_spend_col" as="th">Monthly platform spend</T><T k="official_usage_col" as="th">Approx. official API usage</T></tr></thead>
          <tbody>{tiers.map(([name, discount, localSpend, officialUsage]) => <tr key={name}><T k={name} as="td">Tier</T><td><strong>{discount}</strong></td><td>{localSpend}</td><td>{officialUsage}</td></tr>)}</tbody>
        </table>
      </div>
      <div className="tier-cards">
        {tiers.map(([name, discount, localSpend, officialUsage]) => <div className="tier-mobile" key={name}>
          <div className="tier-mobile-head"><T k={name} as="strong">Tier</T><b>{discount}</b></div>
          <div className="tier-mobile-row"><T k="local_spend_col">Monthly platform spend</T><span>{localSpend}</span></div>
          <div className="tier-mobile-row"><T k="official_usage_col">Approx. official API usage</T><span>{officialUsage}</span></div>
        </div>)}
      </div>
      <T k="tier_footnote" as="p" className="tier-footnote">Displayed official API usage equals monthly platform spend ÷ the share paid after discount. Values are rounded only for display; billing remains exact.</T>
    </div>
  </div>;
}
