import Link from "next/link";
import { FLAT_DISCOUNT_PERCENT } from "@/lib/pricing-tiers";
import { claudeModels, formatUsd, openaiModels, priceFrom } from "@/lib/models";
import { T } from "./translated";
import { TopUpAmountInput } from "./topup-amount-input";

// Плоская модель: одна скидка −50% для всех аккаунтов. Вместо лестницы тиров показываем
// ставку и примеры цен по моделям (official зачёркнуто → ваша цена, из lib/models.ts).
const exampleModels = [
  ...claudeModels.filter((model) => ["claude-opus-4-8", "claude-sonnet-5", "claude-haiku-4-5"].includes(model.id)),
  ...openaiModels.filter((model) => ["gpt-5.6-sol", "gpt-5.6-luna"].includes(model.id)),
];

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
        <div className="tier-heading-copy"><T k="b2c_tag" as="span" className="tag">Flat pricing</T><T k="b2c_h" as="h3">One flat −50% for every account</T></div>
        <div className="tier-rule">
          <div className="tier-rule-item"><T k="tier_rule_keep_label" as="span">Every model</T><T k="tier_rule_keep" as="strong">−{FLAT_DISCOUNT_PERCENT}% off official prices</T></div>
          <div className="tier-rule-item"><T k="tier_rule_miss_label" as="span">No conditions</T><T k="tier_rule_miss" as="strong">Nothing to unlock or maintain</T></div>
        </div>
      </div>
      <div className="tier-table-wrap">
        <table className="tier-table">
          <thead><tr><T k="tier_col" as="th">Model</T><T k="official_col" as="th">Official / 1M in · out</T><T k="discount_col" as="th">Your price / 1M in · out</T></tr></thead>
          <tbody>{exampleModels.map((model) => <tr key={model.id}>
            <td><strong>{model.name}</strong></td>
            <td><s>{formatUsd(model.inputPerM)}</s> · <s>{formatUsd(model.outputPerM)}</s></td>
            <td><strong>{priceFrom(model.inputPerM)}</strong> · <strong>{priceFrom(model.outputPerM)}</strong></td>
          </tr>)}</tbody>
        </table>
      </div>
      <div className="tier-cards">
        {exampleModels.map((model) => <div className="tier-mobile" key={model.id}>
          <div className="tier-mobile-head"><strong>{model.name}</strong><b>−{FLAT_DISCOUNT_PERCENT}%</b></div>
          <div className="tier-mobile-row"><T k="official_col">Official / 1M in · out</T><span><s>{formatUsd(model.inputPerM)}</s> · <s>{formatUsd(model.outputPerM)}</s></span></div>
          <div className="tier-mobile-row"><T k="discount_col">Your price / 1M in · out</T><span>{priceFrom(model.inputPerM)} · {priceFrom(model.outputPerM)}</span></div>
        </div>)}
      </div>
      <T k="tier_footnote" as="p" className="tier-footnote">Every request is metered at the provider's official list price, then billed at half of it. The same flat discount covers every Claude and GPT model, including cache rates.</T>
      <Link className="tier-docs-link" href="/docs#pricing"><T k="tier_docs">Read the full pricing guide</T> →</Link>
    </div>
  </div>;
}
