import {
  OPENKEYS_PRICING_PRODUCT_ID,
  type AccountPolicyBinding,
  type AccountPolicySpec,
  type IssuedEngineApiKey,
} from "@claude-api/contracts";
import {
  assertOpenKeysCatalog,
  assertOpenKeysSwitches,
  buildOfficialOpenKeysPolicy,
  canonicalPricingJson,
  OFFICIAL_ONE_TO_ONE_CONTRACT,
  OFFICIAL_ONE_TO_ONE_MULT_BP,
  officialOpenKeysBinding,
  OpenKeysPolicyError as OpenKeysPricingError,
  type EngineClient,
  type OpenKeysPricingAuthority,
} from "@claude-api/engine-client";

export {
  assertOpenKeysCatalog,
  assertOpenKeysSwitches,
  buildOfficialOpenKeysPolicy,
  OFFICIAL_ONE_TO_ONE_CONTRACT,
  OFFICIAL_ONE_TO_ONE_MULT_BP,
  OpenKeysPricingError,
  type OpenKeysPricingAuthority,
};
const PRICING_OVERRIDE_FIELDS = new Set([
  "discount",
  "discount_bps",
  "discountBps",
  "mult_bp",
  "multBp",
  "multiplier",
  "multiplier_bp",
  "multiplierBp",
  "pricing_contract",
  "pricingContract",
]);

/** API and direct service callers cannot smuggle an alternative economic contract. */
export function assertNoOpenKeysPricingOverride(input: object): void {
  const override = Object.keys(input).find((field) => PRICING_OVERRIDE_FIELDS.has(field));
  if (override !== undefined) {
    throw new OpenKeysPricingError(
      "pricing_override_forbidden",
      `OpenKeys pricing is fixed at 1:1; field ${override} is not accepted`,
    );
  }
}

export function assertOfficialEngineAccount(account: { account: string; multBp: number }): void {
  if (account.multBp !== OFFICIAL_ONE_TO_ONE_MULT_BP) {
    throw new OpenKeysPricingError(
      "engine_multiplier_mismatch",
      "engine did not create the OpenKeys account with the fixed 1:1 multiplier",
    );
  }
}

type OpenKeysPricingEngine = Pick<
  EngineClient,
  | "activateAccountPolicy"
  | "creditAccount"
  | "getAccountPricingState"
  | "getActiveAccountPolicy"
  | "getActivePricingCatalog"
  | "getActiveProviderSwitches"
  | "issueKey"
  | "prepareAccountPolicy"
>;

/** Read-only authority check: this application never invents or overwrites global switches. */
export async function resolveOpenKeysPricingAuthority(
  engine: OpenKeysPricingEngine,
): Promise<OpenKeysPricingAuthority> {
  const [catalog, switches] = await Promise.all([
    engine.getActivePricingCatalog(OPENKEYS_PRICING_PRODUCT_ID),
    engine.getActiveProviderSwitches(),
  ]);
  if (catalog === null || switches === null) {
    throw new OpenKeysPricingError(
      "pricing_authority_missing",
      "OpenKeys product catalog and provider switches must be active before issuance",
    );
  }
  assertOpenKeysCatalog(catalog);
  assertOpenKeysSwitches(switches, catalog);
  return { catalog, switches };
}

function assertMutationAccepted(ack: { result: string }, phase: string): void {
  if (ack.result === "rejected") {
    throw new OpenKeysPricingError(
      "policy_ack_rejected",
      `engine rejected the OpenKeys ${phase} policy ACK`,
    );
  }
}

function policyMatches(
  observed: { policy: AccountPolicySpec; binding: AccountPolicyBinding } | null,
  policy: AccountPolicySpec,
  binding: AccountPolicyBinding,
): boolean {
  return observed !== null &&
    canonicalPricingJson(observed.policy) === canonicalPricingJson(policy) &&
    canonicalPricingJson(observed.binding) === canonicalPricingJson(binding);
}

/** Exact prepare/activate/readback ACK. No credit or secret exists before this returns. */
export async function activateOfficialOpenKeysPolicy(
  engine: OpenKeysPricingEngine,
  accountId: string,
  authority: OpenKeysPricingAuthority,
): Promise<AccountPolicySpec> {
  const policy = buildOfficialOpenKeysPolicy(accountId, authority);
  const binding = officialOpenKeysBinding();
  const prepared = await engine.prepareAccountPolicy(policy);
  assertMutationAccepted(prepared, "prepare");

  const state = await engine.getAccountPricingState(accountId);
  if (typeof state === "object" && "active" in state) {
    const active = await engine.getActiveAccountPolicy(accountId);
    if (policyMatches(active, policy, binding)) return policy;
    throw new OpenKeysPricingError(
      "account_policy_already_bound",
      "new OpenKeys account is already bound to a different policy",
    );
  }
  if (state !== "unbound") {
    throw new OpenKeysPricingError(
      "account_policy_not_unbound",
      "new OpenKeys account has an unexpected inactive policy binding",
    );
  }

  const activated = await engine.activateAccountPolicy(policy, binding, "unbound");
  assertMutationAccepted(activated, "activation");
  const active = await engine.getActiveAccountPolicy(accountId);
  if (!policyMatches(active, policy, binding)) {
    throw new OpenKeysPricingError(
      "policy_ack_readback_mismatch",
      "engine policy ACK did not durably read back with the exact requested identity",
    );
  }
  return policy;
}

/** Policy ACK precedes exact face-value funding, and funding precedes the usable secret. */
export async function provisionOfficialOpenKeysCredential(
  engine: OpenKeysPricingEngine,
  input: {
    accountId: string;
    authority: OpenKeysPricingAuthority;
    faceValueNano: bigint;
    creditReference: string;
    keyLabel: string;
    onCredited?: () => Promise<void>;
  },
): Promise<IssuedEngineApiKey> {
  if (input.faceValueNano <= 0n) {
    throw new OpenKeysPricingError("invalid_face_value", "OpenKeys face value must be positive");
  }
  await activateOfficialOpenKeysPolicy(engine, input.accountId, input.authority);
  await engine.creditAccount(input.accountId, input.faceValueNano, input.creditReference);
  await input.onCredited?.();
  return engine.issueKey(input.accountId, { label: input.keyLabel });
}
