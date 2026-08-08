import type {
  PricingReleaseAssignmentExtensionV2,
  PricingReleasePolicyV2,
  PricingReleaseProvisioningContextV2,
  PricingReleaseProvisioningReleaseV2,
  PricingReleaseV2,
} from "@claude-api/contracts";
import { describe, expect, it, vi } from "vitest";
import {
  buildServicePricingReleasePolicyV2,
  ensureServicePricingReleaseProvisioningV2,
  type PricingReleaseProvisioningTransportV2,
} from "./release-provisioning.js";

const digest = (seed: string): string => `sha256:v2:${seed.repeat(64)}`;

function projection(
  generation: number,
  releaseKind: "target" | "recovery",
  contentDigest: string,
): PricingReleaseProvisioningReleaseV2 {
  return {
    generation,
    release_kind: releaseKind,
    schema_version: 2,
    capability_generation: 3,
    capability_digest: digest("a"),
    main_catalog_generation: 3,
    main_catalog_digest: digest("b"),
    openkeys_catalog_generation: 3,
    openkeys_catalog_digest: digest("c"),
    switch_generation: 3,
    switch_digest: digest("d"),
    inventory_digest: digest("e"),
    funding_manifest_digest: digest("f"),
    minimum_runtime_schema_version: 2,
    content_digest: contentDigest,
  };
}

const target = projection(10, "target", digest("1"));
const recovery = projection(11, "recovery", digest("2"));

function targetContext(): PricingReleaseProvisioningContextV2 {
  return {
    head: {
      active_generation: target.generation,
      active_digest: target.content_digest,
      head_version: 1,
      updated_ts: 1_000,
    },
    activation: {
      activation_id: "1",
      activation_kind: "cutover",
      evidence_digest: digest("3"),
      activated_ts: 1_000,
    },
    active_release: target,
    paired_recovery: {
      release: recovery,
      recovery_link: {
        target_generation: target.generation,
        target_digest: target.content_digest,
        recovery_generation: recovery.generation,
        recovery_digest: recovery.content_digest,
        link_digest: digest("4"),
      },
    },
  };
}

function recoveryContext(): PricingReleaseProvisioningContextV2 {
  return {
    head: {
      active_generation: recovery.generation,
      active_digest: recovery.content_digest,
      head_version: 2,
      updated_ts: 2_000,
    },
    activation: {
      activation_id: "2",
      activation_kind: "recovery",
      evidence_digest: digest("5"),
      activated_ts: 2_000,
    },
    active_release: recovery,
    paired_recovery: null,
  };
}

function fullRelease(item: PricingReleaseProvisioningReleaseV2): PricingReleaseV2 {
  return {
    ...item,
    policy_manifest_digest: digest("6"),
    assignment_manifest_digest: digest("7"),
    assignments: [],
  };
}

function fakeTransport(contexts: PricingReleaseProvisioningContextV2[]) {
  const policies = new Map<string, PricingReleasePolicyV2>();
  const extensions = new Map<string, PricingReleaseAssignmentExtensionV2>();
  const trace: string[] = [];
  let contextRead = 0;
  const currentContext = (): PricingReleaseProvisioningContextV2 =>
    contexts[Math.min(contextRead, contexts.length - 1)]!;
  const engine = {
    getPricingReleaseProvisioningContextV2: vi.fn(async () => {
      trace.push("context");
      const context = currentContext();
      contextRead += 1;
      return structuredClone(context);
    }),
    getPricingReleaseV2: vi.fn(async (generation: number) => {
      trace.push("release");
      if (generation === target.generation) return fullRelease(target);
      if (generation === recovery.generation) return fullRelease(recovery);
      return null;
    }),
    preparePricingReleasePolicyV2: vi.fn(async (policy: PricingReleasePolicyV2) => {
      trace.push("policy-prepare");
      policies.set(`${policy.policy_id}:${policy.policy_version}`, structuredClone(policy));
      return {
        result: "stored" as const,
        identity: {
          policy_id: policy.policy_id,
          policy_version: policy.policy_version,
          content_digest: policy.content_digest,
        },
      } as never;
    }),
    getPricingReleasePolicyV2: vi.fn(async (policyId: string, policyVersion: number) => {
      trace.push("policy-readback");
      return policies.get(`${policyId}:${policyVersion}`) ?? null;
    }),
    preparePricingReleaseAssignmentExtensionV2: vi.fn(async (
      extension: PricingReleaseAssignmentExtensionV2,
    ) => {
      trace.push("extension-prepare");
      const accountId = extension.members[0]!.assignment.account_id;
      extensions.set(`${extension.provisioning_head_version}:${accountId}`, structuredClone(extension));
      return {
        result: "stored" as const,
        identity: {
          provisioning_head_generation: extension.provisioning_head_generation,
          provisioning_head_version: extension.provisioning_head_version,
          account_id: accountId,
          extension_group_digest: extension.extension_group_digest,
        },
      } as never;
    }),
    getPricingReleaseAssignmentExtensionV2: vi.fn(async (headVersion: number, accountId: string) => {
      trace.push("extension-readback");
      return extensions.get(`${headVersion}:${accountId}`) ?? null;
    }),
  } as unknown as PricingReleaseProvisioningTransportV2;
  return { engine, policies, extensions, trace };
}

const serviceInput = {
  accountId: "acct_new",
  serviceId: "crm-parsing",
  purpose: "CRM ingestion",
  responsible: "platform",
  releaseRequired: true,
} as const;

describe("release-v2 service provisioning", () => {
  it("versions service policies by admitted capability generation", () => {
    expect(buildServicePricingReleasePolicyV2(targetContext(), "worker").policy_version).toBe(1);

    const generation5 = targetContext();
    generation5.active_release = { ...generation5.active_release, capability_generation: 5 };
    expect(buildServicePricingReleasePolicyV2(generation5, "worker").policy_version).toBe(2);

    const generation6 = targetContext();
    generation6.active_release = { ...generation6.active_release, capability_generation: 6 };
    expect(buildServicePricingReleasePolicyV2(generation6, "worker").policy_version).toBe(3);

    const rejected = targetContext();
    rejected.active_release = { ...rejected.active_release, capability_generation: 4 };
    expect(() => buildServicePricingReleasePolicyV2(rejected, "worker"))
      .toThrow("pricing_release_rejected_capability_generation");
  });

  it("creates a single rule-free meter-only member under an active recovery", async () => {
    const state = fakeTransport([recoveryContext()]);
    await expect(ensureServicePricingReleaseProvisioningV2(state.engine, serviceInput))
      .resolves.toEqual({ status: "extension", headVersion: 2, releaseGeneration: 11 });

    const policy = [...state.policies.values()][0]!;
    expect(policy).toMatchObject({
      owner_type: "service",
      owner_id: "crm-parsing",
      account_class: "service",
      product_id: null,
      billing_mode: "meter_only",
      catalog_generation: null,
      switch_generation: null,
      rules: [],
    });
    const extension = [...state.extensions.values()][0]!;
    expect(extension).toMatchObject({
      paired_recovery_generation: null,
      paired_recovery_digest: null,
      members: [{
        release_generation: 11,
        assignment: {
          billing_mode: "meter_only",
          funding_generation: null,
          purpose: "CRM ingestion",
          responsible: "platform",
        },
      }],
    });
    expect(state.trace).toEqual([
      "context",
      "release",
      "policy-prepare",
      "policy-readback",
      "extension-prepare",
      "extension-readback",
      "context",
    ]);
  });

  it("stores the exact target/recovery pair under an active target", async () => {
    const state = fakeTransport([targetContext()]);
    await expect(ensureServicePricingReleaseProvisioningV2(state.engine, serviceInput))
      .resolves.toEqual({ status: "extension", headVersion: 1, releaseGeneration: 10 });
    const extension = [...state.extensions.values()][0]!;
    expect(extension.members.map((member) => member.release_generation)).toEqual([10, 11]);
  });

  it("accepts a target extension when the final fresh context advances to its paired recovery", async () => {
    const state = fakeTransport([targetContext(), recoveryContext()]);
    await expect(ensureServicePricingReleaseProvisioningV2(state.engine, serviceInput))
      .resolves.toEqual({ status: "extension", headVersion: 2, releaseGeneration: 11 });
    expect(state.extensions.size).toBe(1);
  });

  it("fails closed when exact extension readback does not match", async () => {
    const state = fakeTransport([targetContext()]);
    vi.mocked(state.engine.getPricingReleaseAssignmentExtensionV2).mockResolvedValue(null);
    await expect(ensureServicePricingReleaseProvisioningV2(state.engine, serviceInput))
      .rejects.toMatchObject({ code: "assignment_conflict" });
  });
});
