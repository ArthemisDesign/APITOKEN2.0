import Link from "next/link";
import { T } from "./translated";

const tiers = [
  ["tier_starter", "60%", "$0", "$0"],
  ["tier_builder", "65%", "$25", "$60+"],
  ["tier_pro", "70%", "$75", "$200+"],
  ["tier_studio", "75%", "$200", "$600+"],
  ["tier_scale", "80%", "$500", "$1,800+"],
] as const;

export function PricingOverview() {
  return <div className="pricing-overview">
    <div className="pricing-intro" data-reveal-stagger>
      <div className="topup-card">
        <T k="topup_tag" as="span" className="tag">Flexible top-up</T>
        <T k="topup_h" as="h3">Choose any whole USD amount</T>
        <div className="topup-preview" aria-label="Example amount"><span>$</span><b>100</b></div>
        <T k="topup_p" as="p">No catalog and no preset amounts. Enter a whole amount and add exactly that much to your balance.</T>
        <Link className="btn btn-primary" href="/register"><T k="topup_cta">Create account</T></Link>
      </div>
      <div className="business-card">
        <T k="b2b_tag" as="span" className="tag">B2B · Invite only</T>
        <T k="b2b_h" as="h3">Negotiated business pricing</T>
        <T k="b2b_p" as="p">Business customers receive an operator-set discount under an individual agreement and join through a private invitation.</T>
        <T k="b2b_note" as="span" className="business-note">Custom rates · consolidated access · direct onboarding</T>
      </div>
    </div>
    <div className="tier-section">
      <div className="tier-heading">
        <div><T k="b2c_tag" as="span" className="tag">B2C · Progressive pricing</T><T k="b2c_h" as="h3">Your discount grows with monthly usage</T></div>
        <T k="tier_rule" as="p">Your achieved tier carries into the next calendar month. If you miss its target, you move down only one tier.</T>
      </div>
      <div className="tier-table-wrap">
        <table className="tier-table">
          <thead><tr><T k="tier_col" as="th">Tier</T><T k="discount_col" as="th">Discount</T><T k="local_spend_col" as="th">Monthly platform spend</T><T k="official_usage_col" as="th">Approx. official API usage</T></tr></thead>
          <tbody>{tiers.map(([name, discount, localSpend, officialUsage]) => <tr key={name}><T k={name} as="td">Tier</T><td><strong>{discount}</strong></td><td>{localSpend}</td><td>{officialUsage}</td></tr>)}</tbody>
        </table>
      </div>
      <T k="tier_footnote" as="p" className="tier-footnote">Progress uses authoritative balance spent during the UTC calendar month. Official usage values are rounded client-facing equivalents.</T>
    </div>
  </div>;
}
