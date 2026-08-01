"use client";

import { Pill } from "@/components/ui";

export interface PricingRule {
  scope: { provider: { providerId: string } } | { model: { providerId: string; canonicalModelId: string } };
  pricingMode: "track" | "discount";
  discountBps: number | null;
}

export interface PricingCatalogView {
  productId: string;
  catalogGeneration: number;
  switchGeneration: number;
  switchDigest: string;
  switchSyncState: "pending" | "processing" | "retry" | "confirmed" | "superseded" | "dead" | "missing";
  switchLastError: string | null;
  providers: Array<{
    providerId: string;
    masterEnabled: boolean;
    productEnabled: boolean;
    b2cEnabled: boolean;
    b2bEnabled: boolean;
    models: string[];
  }>;
}

export interface ManagedPolicyView {
  policyId: string;
  ownerType: "global_b2c" | "b2b_client" | "b2b_invitation" | "service";
  ownerId: string;
  productId: string;
  currentVersion: number;
  currentDigest: string;
  catalogGeneration: number;
  currentActorType?: string;
  currentActorId?: string | null;
  currentReason?: string;
  currentCreatedAt?: string;
  servicePurpose?: string | null;
  serviceResponsible?: string | null;
  rules: PricingRule[];
  targets: Array<{
    bindingId: string;
    accountId: string;
    accountClass: "b2c" | "b2b" | "service";
    desiredVersion: number | null;
    appliedVersion: number | null;
    syncState: "legacy" | "pending" | "confirmed" | "failed";
    deliveryState?: "pending" | "processing" | "retry" | "confirmed" | "superseded" | "dead" | "missing";
    lastError: string | null;
  }>;
}

export function canonicalizePricingRules(rules: readonly PricingRule[]): PricingRule[] {
  return [...rules]
    .map((rule) => ({
      scope: "provider" in rule.scope
        ? { provider: { providerId: rule.scope.provider.providerId } }
        : {
            model: {
              providerId: rule.scope.model.providerId,
              canonicalModelId: rule.scope.model.canonicalModelId,
            },
          },
      pricingMode: rule.pricingMode,
      discountBps: rule.discountBps,
    } satisfies PricingRule))
    .sort((left, right) => {
      const leftKey = scopeKey(left.scope);
      const rightKey = scopeKey(right.scope);
      return leftKey < rightKey ? -1 : leftKey > rightKey ? 1 : 0;
    });
}

export function pricingRulesSignature(rules: readonly PricingRule[]): string {
  return JSON.stringify(canonicalizePricingRules(rules));
}

export function ManagedPolicyEditor(props: {
  catalog: PricingCatalogView;
  policy: ManagedPolicyView;
  rules: PricingRule[];
  onRulesChange: (rules: PricingRule[]) => void;
  allowTrack?: boolean;
  segment: "b2c" | "b2b" | "service";
  disabled?: boolean;
}) {
  return (
    <>
      <div className="policy-meta">
        <Pill kind="info">source v{props.policy.currentVersion}</Pill>
        <span>catalog g{props.policy.catalogGeneration}</span>
        <code>{props.policy.currentDigest}</code>
        {props.policy.currentActorType ? (
          <span>
            {props.policy.currentActorType}:{props.policy.currentActorId ?? "system"}
            {props.policy.currentReason ? ` · ${props.policy.currentReason}` : ""}
          </span>
        ) : null}
        {props.policy.currentCreatedAt ? (
          <time dateTime={props.policy.currentCreatedAt}>{props.policy.currentCreatedAt}</time>
        ) : null}
        {props.policy.servicePurpose ? <span>назначение: {props.policy.servicePurpose}</span> : null}
        {props.policy.serviceResponsible ? <span>ответственный: {props.policy.serviceResponsible}</span> : null}
      </div>
      <PolicyRuleEditor
        catalog={props.catalog}
        rules={props.rules}
        onChange={props.onRulesChange}
        allowTrack={props.allowTrack}
        segment={props.segment}
        disabled={props.disabled}
      />
      <PolicyTargets targets={props.policy.targets} />
    </>
  );
}

export function PolicyTargets(props: { targets: ManagedPolicyView["targets"] }) {
  if (props.targets.length === 0) {
    return <div className="policy-rule-count">Snapshot ещё не привязан к engine account.</div>;
  }
  return (
    <div className="policy-targets">
      {props.targets.map((target) => (
        <div className="policy-target" key={target.bindingId}>
          <div>
            <code>{target.accountId}</code>
            {target.lastError ? <div className="sub">{target.lastError}</div> : null}
          </div>
          <span>desired {target.desiredVersion == null ? "—" : `v${target.desiredVersion}`}</span>
          <span>applied {target.appliedVersion == null ? "—" : `v${target.appliedVersion}`}</span>
          <Pill kind={target.deliveryState === "confirmed" ? "ok" : target.deliveryState === "dead" ? "bad" : "warn"}>
            {target.deliveryState ?? target.syncState}
          </Pill>
        </div>
      ))}
    </div>
  );
}

export function PolicyRuleEditor(props: {
  catalog: PricingCatalogView;
  rules: PricingRule[];
  onChange: (rules: PricingRule[]) => void;
  allowTrack?: boolean;
  segment: "b2c" | "b2b" | "service";
  disabled?: boolean;
}) {
  const setRule = (scope: PricingRule["scope"], enabled: boolean) => {
    const key = scopeKey(scope);
    const remaining = props.rules.filter((rule) => scopeKey(rule.scope) !== key);
    if (!enabled) return props.onChange(remaining);
    props.onChange([...remaining, {
      scope,
      pricingMode: props.allowTrack ? "track" : "discount",
      discountBps: props.allowTrack ? null : 0,
    }]);
  };
  const replaceRule = (scope: PricingRule["scope"], change: Partial<PricingRule>) => {
    const key = scopeKey(scope);
    props.onChange(props.rules.map((rule) => scopeKey(rule.scope) === key ? { ...rule, ...change } : rule));
  };

  return (
    <div className="policy-editor">
      <div className="policy-editor-head">
        <div>
          <b>Правила продукта {props.catalog.productId}</b>
          <span>catalog g{props.catalog.catalogGeneration} · switches g{props.catalog.switchGeneration}</span>
        </div>
        <Pill kind={props.catalog.switchSyncState === "confirmed" ? "ok" : props.catalog.switchSyncState === "dead" ? "bad" : "warn"}>
          switches {props.catalog.switchSyncState}
        </Pill>
      </div>
      {props.catalog.providers.map((provider) => {
        const providerScope = { provider: { providerId: provider.providerId } } as const;
        const providerRule = findRule(props.rules, providerScope);
        const segmentEnabled = props.segment === "service"
          ? true
          : props.segment === "b2c"
            ? provider.b2cEnabled
            : provider.b2bEnabled;
        const available = provider.masterEnabled && provider.productEnabled && segmentEnabled;
        return (
          <section className="policy-provider" key={provider.providerId}>
            <div className="policy-provider-head">
              <label className="policy-rule-toggle">
                <input
                  type="checkbox"
                  checked={Boolean(providerRule)}
                  disabled={props.disabled}
                  onChange={(event) => setRule(providerScope, event.target.checked)}
                />
                <span><b>{provider.providerId}</b><small>правило на все включённые модели</small></span>
              </label>
              <Pill kind={available ? "ok" : "warn"}>{available ? "switch gates active" : "выключен switch"}</Pill>
            </div>
            {providerRule ? (
              <>
                <RuleControls
                  rule={providerRule}
                  allowTrack={props.allowTrack}
                  disabled={props.disabled}
                  onChange={(change) => replaceRule(providerScope, change)}
                />
                <div className="policy-provider-warning">
                  Provider default охватывает только модели этой catalog generation; будущая модель появится лишь после явного catalog enablement.
                </div>
              </>
            ) : null}
            <div className="policy-models">
              {provider.models.map((modelId) => {
                const modelScope = { model: { providerId: provider.providerId, canonicalModelId: modelId } } as const;
                const modelRule = findRule(props.rules, modelScope);
                const effectiveRule = modelRule ?? providerRule;
                return (
                  <div className="policy-model" key={modelId}>
                    <label className="policy-rule-toggle">
                      <input
                        type="checkbox"
                        checked={Boolean(modelRule)}
                        disabled={props.disabled}
                        onChange={(event) => setRule(modelScope, event.target.checked)}
                      />
                      <span><b>{modelId}</b><small>точное переопределение</small></span>
                    </label>
                    <div className="policy-model-config">
                      <Pill kind={available && effectiveRule ? "ok" : "warn"}>
                        {!available
                          ? "недоступна: switch"
                          : !effectiveRule
                            ? "недоступна: нет rule"
                            : rulePreview(effectiveRule, modelRule ? "model override" : "provider default")}
                      </Pill>
                      {modelRule ? (
                        <RuleControls
                          rule={modelRule}
                          allowTrack={props.allowTrack}
                          disabled={props.disabled}
                          compact
                          onChange={(change) => replaceRule(modelScope, change)}
                        />
                      ) : null}
                    </div>
                  </div>
                );
              })}
            </div>
          </section>
        );
      })}
      <div className={props.rules.length ? "policy-rule-count" : "policy-rule-count bad"}>
        {props.rules.length ? `${props.rules.length} правил в полной replacement policy` : "Нужно выбрать хотя бы одно правило"}
      </div>
    </div>
  );
}

function RuleControls(props: {
  rule: PricingRule;
  allowTrack?: boolean;
  disabled?: boolean;
  compact?: boolean;
  onChange: (change: Partial<PricingRule>) => void;
}) {
  return (
    <div className={"policy-rule-controls" + (props.compact ? " compact" : "")}>
      <label>
        <span>режим</span>
        <select
          value={props.rule.pricingMode}
          disabled={props.disabled}
          onChange={(event) => props.onChange(event.target.value === "track"
            ? { pricingMode: "track", discountBps: null }
            : { pricingMode: "discount", discountBps: 0 })}
        >
          {props.allowTrack ? <option value="track">прогрессивный тариф</option> : null}
          <option value="discount">фиксированная скидка</option>
        </select>
      </label>
      {props.rule.pricingMode === "discount" ? (
        <label>
          <span>скидка, %</span>
          <input
            type="number"
            min={0}
            max={95}
            step={1}
            value={(props.rule.discountBps ?? 0) / 100}
            disabled={props.disabled}
            onChange={(event) => {
              const value = Number(event.target.value);
              if (Number.isInteger(value) && value >= 0 && value <= 95) props.onChange({ discountBps: value * 100 });
            }}
          />
        </label>
      ) : (
        <span className="policy-progressive-note">множитель материализуется из текущего B2C-тира</span>
      )}
    </div>
  );
}

function scopeKey(scope: PricingRule["scope"]): string {
  return "provider" in scope
    ? `${scope.provider.providerId}\0`
    : `${scope.model.providerId}\0${scope.model.canonicalModelId}`;
}

function findRule(rules: PricingRule[], scope: PricingRule["scope"]): PricingRule | undefined {
  const key = scopeKey(scope);
  return rules.find((rule) => scopeKey(rule.scope) === key);
}

function rulePreview(rule: PricingRule, source: string): string {
  return rule.pricingMode === "track"
    ? `${source}: прогрессивный тариф`
    : `${source}: скидка ${(rule.discountBps ?? 0) / 100}%`;
}
