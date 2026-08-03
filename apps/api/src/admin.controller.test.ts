import { BadRequestException, HttpException, NotFoundException } from "@nestjs/common";
import {
  FundingNormalizationJobV2Error,
  PricingPolicyWriteError,
  PricingReleaseActivationJobV2Error,
  PricingStage8CaptureJobV2Error,
  ServiceAccountInventoryV2Error,
  Stage5MaterializerV2Error,
} from "@claude-api/db";
import { describe, expect, it, vi } from "vitest";
import { AdminController } from "./admin.controller.js";
import { AdminServiceAccountInventoryError, type AdminService } from "./admin.service.js";

describe("admin user list HTTP contract", () => {
  it("passes bounded pagination and filters to the service", async () => {
    const listUsers = vi.fn().mockResolvedValue({ users: [], total: 0, limit: 25, offset: 50 });
    const controller = new AdminController({ listUsers } as unknown as AdminService);

    await expect(controller.listUsers("25", "50", "alice", "active", "google", "b2b"))
      .resolves.toMatchObject({ total: 0 });
    expect(listUsers).toHaveBeenCalledWith({
      limit: 25,
      offset: 50,
      sort: "created_at",
      dir: "desc",
      search: "alice",
      status: "active",
      auth: "google",
      customerType: "b2b",
    });
  });

  it("passes a whitelisted sort and direction to the service", async () => {
    const listUsers = vi.fn().mockResolvedValue({ users: [], total: 0, limit: 50, offset: 0 });
    const controller = new AdminController({ listUsers } as unknown as AdminService);

    await expect(controller.listUsers(undefined, undefined, undefined, undefined, undefined, undefined, "paid_total", "asc"))
      .resolves.toMatchObject({ total: 0 });
    expect(listUsers).toHaveBeenCalledWith({ limit: 50, offset: 0, sort: "paid_total", dir: "asc" });
  });

  it("rejects unbounded or unknown filters and non-whitelisted sorts", async () => {
    const listUsers = vi.fn();
    const controller = new AdminController({ listUsers } as unknown as AdminService);
    await expect(controller.listUsers("500", "0")).rejects.toBeInstanceOf(BadRequestException);
    await expect(controller.listUsers("50", "0", "", "unknown")).rejects.toBeInstanceOf(BadRequestException);
    // sort интерполируется в ORDER BY — принимаются только значения белого списка.
    await expect(controller.listUsers(undefined, undefined, undefined, undefined, undefined, undefined, "balance_usd"))
      .rejects.toBeInstanceOf(BadRequestException);
    await expect(controller.listUsers(undefined, undefined, undefined, undefined, undefined, undefined, "created_at; DROP TABLE users;--"))
      .rejects.toBeInstanceOf(BadRequestException);
    await expect(controller.listUsers(undefined, undefined, undefined, undefined, undefined, undefined, "spent_30d", "sideways"))
      .rejects.toBeInstanceOf(BadRequestException);
    expect(listUsers).not.toHaveBeenCalled();
  });

  it("accepts copy-only invitations and forwards the verified operator identity", async () => {
    const createBusinessInvite = vi.fn().mockResolvedValue({
      id: "invite-id",
      email: null,
      deliveryStatus: "copy_only",
    });
    const controller = new AdminController({ createBusinessInvite } as unknown as AdminService);
    const body = {
      discountPercent: 75,
      expiresInDays: 7,
      reason: "negotiated business terms",
      idempotencyKey: "2d20f96d-0f2b-4cff-9fa0-7c4b7fe1a6c5",
    };

    await expect(controller.createBusinessInvite(body, "owner@example.com"))
      .resolves.toMatchObject({ deliveryStatus: "copy_only" });
    expect(createBusinessInvite).toHaveBeenCalledWith({
      ...body,
      actorId: "owner@example.com",
    });
  });

  it("accepts a full provider/model invitation policy without a scalar discount", async () => {
    const createBusinessInvite = vi.fn().mockResolvedValue({ id: "invite-id", policy: { currentVersion: 1 } });
    const controller = new AdminController({ createBusinessInvite } as unknown as AdminService);
    const body = {
      policy: {
        rules: [{
          scope: { provider: { providerId: "anthropic" } },
          pricingMode: "discount",
          discountBps: 6_000,
        }],
      },
      expiresInDays: 7,
      reason: "negotiated provider policy",
      idempotencyKey: "2d20f96d-0f2b-4cff-9fa0-7c4b7fe1a6c5",
    };

    await expect(controller.createBusinessInvite(body, "owner@example.com"))
      .resolves.toMatchObject({ policy: { currentVersion: 1 } });
    expect(createBusinessInvite).toHaveBeenCalledWith({ ...body, actorId: "owner@example.com" });
  });

  it("rejects ambiguous mutations that combine full policy and scalar compatibility fields", async () => {
    const createBusinessInvite = vi.fn();
    const setBusinessPricing = vi.fn();
    const controller = new AdminController({ createBusinessInvite, setBusinessPricing } as unknown as AdminService);
    const policy = {
      rules: [{
        scope: { provider: { providerId: "anthropic" } },
        pricingMode: "discount",
        discountBps: 6_000,
      }],
    };
    await expect(controller.createBusinessInvite({
      discountPercent: 60,
      policy,
      expiresInDays: 7,
      reason: "ambiguous rolling payload",
      idempotencyKey: "2d20f96d-0f2b-4cff-9fa0-7c4b7fe1a6c5",
    })).rejects.toBeInstanceOf(BadRequestException);
    await expect(controller.setBusinessPricing(
      "4f53639f-ced1-472f-998e-50e426bd5734",
      {
        discountPercent: 60,
        policy: { expectedVersion: 1, rules: policy.rules },
        reason: "ambiguous rolling payload",
      },
    )).rejects.toBeInstanceOf(BadRequestException);
    expect(createBusinessInvite).not.toHaveBeenCalled();
    expect(setBusinessPricing).not.toHaveBeenCalled();
  });
});

describe("managed pricing HTTP contract", () => {
  const rule = {
    scope: { provider: { providerId: "anthropic" } },
    pricingMode: "discount" as const,
    discountBps: 6_000,
  };

  it("forwards switch CAS and the bounded admin actor", async () => {
    const updateManagedProviderSwitches = vi.fn().mockResolvedValue({ switchGeneration: 3 });
    const controller = new AdminController({ updateManagedProviderSwitches } as unknown as AdminService);
    const body = {
      expectedGeneration: 2,
      reason: "disable only the B2B segment",
      providers: [{
        providerId: "anthropic",
        masterEnabled: true,
        productEnabled: true,
        b2cEnabled: true,
        b2bEnabled: false,
      }],
    };

    await expect(controller.updateManagedProviderSwitches(body, "operator@example.com"))
      .resolves.toMatchObject({ switchGeneration: 3 });
    expect(updateManagedProviderSwitches).toHaveBeenCalledWith(body, "operator@example.com");
  });

  it("wraps a B2B replacement policy in the existing rolling-compatible endpoint", async () => {
    const setBusinessPricing = vi.fn().mockResolvedValue({ policy: { currentVersion: 4 } });
    const controller = new AdminController({ setBusinessPricing } as unknown as AdminService);
    const userId = "4f53639f-ced1-472f-998e-50e426bd5734";
    const body = {
      policy: { expectedVersion: 3, rules: [rule] },
      reason: "replace the complete negotiated policy",
    };

    await expect(controller.setBusinessPricing(userId, body, "operator@example.com"))
      .resolves.toMatchObject({ policy: { currentVersion: 4 } });
    expect(setBusinessPricing).toHaveBeenCalledWith(
      userId,
      { policy: body.policy },
      "operator@example.com",
      body.reason,
    );
  });

  it("lists service policies without accepting inferred owner input", async () => {
    const listManagedServicePricingPolicies = vi.fn().mockResolvedValue({ policies: [{ ownerId: "crm" }] });
    const controller = new AdminController({ listManagedServicePricingPolicies } as unknown as AdminService);

    await expect(controller.listServicePricingPolicies()).resolves.toEqual({ policies: [{ ownerId: "crm" }] });
    expect(listManagedServicePricingPolicies).toHaveBeenCalledOnce();
  });

  it("runs Stage 5 only with an explicit operator and materializes the exact reviewed digest", async () => {
    const planDigest = `sha256:v2:${"a".repeat(64)}`;
    const dryRunPricingStage5V2 = vi.fn().mockResolvedValue({
      mode: "dry_run",
      status: "dry_run",
      plan_digest: planDigest,
    });
    const materializePricingStage5V2 = vi.fn().mockResolvedValue({
      mode: "apply",
      status: "materializing",
      plan_digest: planDigest,
    });
    const controller = new AdminController({
      dryRunPricingStage5V2,
      materializePricingStage5V2,
    } as unknown as AdminService);

    await expect(controller.dryRunPricingStage5V2({}, "operator@example.test"))
      .resolves.toMatchObject({ status: "dry_run" });
    expect(dryRunPricingStage5V2).toHaveBeenCalledOnce();
    await expect(controller.materializePricingStage5V2({
      plan_digest: planDigest,
      reason: "materialize the reviewed complete inventory",
    }, "operator@example.test")).resolves.toMatchObject({ status: "materializing" });
    expect(materializePricingStage5V2).toHaveBeenCalledWith({
      plan_digest: planDigest,
      reason: "materialize the reviewed complete inventory",
    }, "operator@example.test");

    await expect(controller.dryRunPricingStage5V2({}, undefined))
      .rejects.toBeInstanceOf(BadRequestException);
    await expect(controller.dryRunPricingStage5V2({ extra: true }, "operator"))
      .rejects.toBeInstanceOf(BadRequestException);
    await expect(controller.materializePricingStage5V2({
      plan_digest: planDigest,
      reason: "x",
    }, "operator")).rejects.toBeInstanceOf(BadRequestException);
  });

  it("reads and stages Stage 6 by the same exact plan digest with attributed intent", async () => {
    const planDigest = `sha256:v2:${"b".repeat(64)}`;
    const getPricingStage6V2 = vi.fn().mockResolvedValue({
      stage5_plan_digest: planDigest,
      job_id: null,
    });
    const stagePricingStage6V2 = vi.fn().mockResolvedValue({
      stage5_plan_digest: planDigest,
      job_status: "pending",
    });
    const controller = new AdminController({
      getPricingStage6V2,
      stagePricingStage6V2,
    } as unknown as AdminService);

    await expect(controller.getPricingStage6V2(planDigest, "operator@example.test"))
      .resolves.toMatchObject({ job_id: null });
    expect(getPricingStage6V2).toHaveBeenCalledWith(planDigest);
    const body = {
      plan_digest: planDigest,
      reason: "start full-inventory funding normalization",
    };
    await expect(controller.stagePricingStage6V2(body, "operator@example.test"))
      .resolves.toMatchObject({ job_status: "pending" });
    expect(stagePricingStage6V2).toHaveBeenCalledWith(body, "operator@example.test");

    await expect(controller.getPricingStage6V2("bad-digest", "operator"))
      .rejects.toBeInstanceOf(BadRequestException);
    await expect(controller.getPricingStage6V2(planDigest, undefined))
      .rejects.toBeInstanceOf(BadRequestException);
    await expect(controller.stagePricingStage6V2({ ...body, extra: true }, "operator"))
      .rejects.toBeInstanceOf(BadRequestException);
  });

  it("maps Stage 5/6 state conflicts to 409 and retryable authority failures to 503", async () => {
    const planDigest = `sha256:v2:${"c".repeat(64)}`;
    const stage5Conflict = new AdminController({
      materializePricingStage5V2: vi.fn().mockRejectedValue(
        new Stage5MaterializerV2Error("expected_plan_stale", "plan changed"),
      ),
    } as unknown as AdminService);
    const stage5Unavailable = new AdminController({
      dryRunPricingStage5V2: vi.fn().mockRejectedValue(
        new Stage5MaterializerV2Error("openkeys_inventory_unavailable", "OpenKeys unavailable"),
      ),
    } as unknown as AdminService);
    const stage6Conflict = new AdminController({
      stagePricingStage6V2: vi.fn().mockRejectedValue(
        new FundingNormalizationJobV2Error("Stage 5 is not materializing", true),
      ),
    } as unknown as AdminService);

    await expect(stage5Conflict.materializePricingStage5V2({
      plan_digest: planDigest,
      reason: "materialize exact reviewed inventory",
    }, "operator")).rejects.toMatchObject({ status: 409 });
    await expect(stage5Unavailable.dryRunPricingStage5V2({}, "operator"))
      .rejects.toMatchObject({ status: 503 });
    await expect(stage6Conflict.stagePricingStage6V2({
      plan_digest: planDigest,
      reason: "stage exact funding normalization",
    }, "operator")).rejects.toMatchObject({ status: 409 });
  });

  it("exposes read-only activation control and stages only an explicit attributed request", async () => {
    const getPricingReleaseActivationControlV2 = vi.fn().mockResolvedValue({
      database_observed_at: "2026-08-03T00:00:00.000Z",
      engine: { available: true, head: null },
    });
    const stagePricingReleaseActivationV2 = vi.fn().mockResolvedValue({
      job_id: "4f53639f-ced1-472f-998e-50e426bd5734",
      status: "accepted",
    });
    const controller = new AdminController({
      getPricingReleaseActivationControlV2,
      stagePricingReleaseActivationV2,
    } as unknown as AdminService);
    const body = {
      activation_kind: "cutover",
      evidence_digest: `sha256:v2:${"a".repeat(64)}`,
      reason: "activate the reviewed full-inventory target",
    };

    await expect(controller.getPricingReleaseActivationControlV2())
      .resolves.toMatchObject({ engine: { available: true, head: null } });
    await expect(controller.stagePricingReleaseActivationV2(body, "operator@example.test"))
      .resolves.toMatchObject({ status: "accepted" });
    expect(stagePricingReleaseActivationV2).toHaveBeenCalledWith(body, "operator@example.test");

    await expect(controller.stagePricingReleaseActivationV2(body))
      .rejects.toBeInstanceOf(BadRequestException);
    await expect(controller.stagePricingReleaseActivationV2({ ...body, extra: true }, "operator"))
      .rejects.toBeInstanceOf(BadRequestException);
  });

  it("maps activation state conflicts to 409 without hiding transient staging failures", async () => {
    const conflict = new AdminController({
      stagePricingReleaseActivationV2: vi.fn().mockRejectedValue(
        new PricingReleaseActivationJobV2Error("evidence expired", true),
      ),
    } as unknown as AdminService);
    const unavailable = new AdminController({
      stagePricingReleaseActivationV2: vi.fn().mockRejectedValue(
        new PricingReleaseActivationJobV2Error("database unavailable", false),
      ),
    } as unknown as AdminService);
    const body = {
      activation_kind: "recovery",
      evidence_digest: `sha256:v2:${"b".repeat(64)}`,
      reason: "activate the exact reviewed recovery release",
    };

    await expect(conflict.stagePricingReleaseActivationV2(body, "operator"))
      .rejects.toMatchObject({ status: 409 });
    await expect(unavailable.stagePricingReleaseActivationV2(body, "operator"))
      .rejects.toMatchObject({ status: 503 });
  });

  it("exposes and explicitly stages managed Stage 8 capture without staging activation", async () => {
    const getPricingStage8CaptureControlV2 = vi.fn().mockResolvedValue({
      database_observed_at: "2026-08-03T00:00:00.000Z",
      counts_by_status: { pending: 0, processing: 0, retry: 0, passed: 0, blocked: 0, dead: 0 },
      jobs: [],
      artifacts: [],
    });
    const stagePricingStage8CaptureV2 = vi.fn().mockResolvedValue({
      job_id: "4f53639f-ced1-472f-998e-50e426bd5734",
      request_digest: `sha256:v2:${"a".repeat(64)}`,
      status: "accepted",
    });
    const stagePricingReleaseActivationV2 = vi.fn();
    const controller = new AdminController({
      getPricingStage8CaptureControlV2,
      stagePricingStage8CaptureV2,
      stagePricingReleaseActivationV2,
    } as unknown as AdminService);
    const body = {
      idempotency_key: "2d20f96d-0f2b-4cff-9fa0-7c4b7fe1a6c5",
      target_generation: 41,
      recovery_generation: 42,
      window_start_ts: 1_785_700_000,
      window_end_ts: 1_785_700_300,
      min_samples_per_provider: 100,
      financial_sample_size: 100,
      gemini_client_admissions: 27,
      reason: "capture the reviewed full-inventory Stage 8 window",
    };

    await expect(controller.getPricingStage8CaptureControlV2())
      .resolves.toMatchObject({ jobs: [], artifacts: [] });
    await expect(controller.stagePricingStage8CaptureV2(body, "operator@example.test"))
      .resolves.toMatchObject({ status: "accepted" });
    expect(stagePricingStage8CaptureV2).toHaveBeenCalledWith(body, "operator@example.test");
    expect(stagePricingReleaseActivationV2).not.toHaveBeenCalled();
    await expect(controller.stagePricingStage8CaptureV2(body))
      .rejects.toBeInstanceOf(BadRequestException);
    await expect(controller.stagePricingStage8CaptureV2({ ...body, extra: true }, "operator"))
      .rejects.toBeInstanceOf(BadRequestException);
  });

  it("maps managed Stage 8 capture conflicts to 409 and transient staging failures to 503", async () => {
    const body = {
      idempotency_key: "2d20f96d-0f2b-4cff-9fa0-7c4b7fe1a6c5",
      target_generation: 41,
      recovery_generation: 42,
      window_start_ts: 1_785_700_000,
      window_end_ts: 1_785_700_300,
      min_samples_per_provider: 100,
      financial_sample_size: 100,
      gemini_client_admissions: 27,
      reason: "capture the reviewed full-inventory Stage 8 window",
    };
    const conflict = new AdminController({
      stagePricingStage8CaptureV2: vi.fn().mockRejectedValue(
        new PricingStage8CaptureJobV2Error("idempotency conflict", true),
      ),
    } as unknown as AdminService);
    const unavailable = new AdminController({
      stagePricingStage8CaptureV2: vi.fn().mockRejectedValue(
        new PricingStage8CaptureJobV2Error("database unavailable", false),
      ),
    } as unknown as AdminService);

    await expect(conflict.stagePricingStage8CaptureV2(body, "operator"))
      .rejects.toMatchObject({ status: 409 });
    await expect(unavailable.stagePricingStage8CaptureV2(body, "operator"))
      .rejects.toMatchObject({ status: 503 });
  });

  it("writes service inventory only through strict CAS metadata and a verified actor", async () => {
    const upsertServiceAccountInventoryV2 = vi.fn().mockResolvedValue({
      status: "stored",
      account: { service_id: "crm-parsing", source_version: 1 },
    });
    const controller = new AdminController({ upsertServiceAccountInventoryV2 } as unknown as AdminService);
    const body = {
      expected_source_version: null,
      expected_content_digest: null,
      engine_account_id: "acct_service_crm",
      purpose: "CRM ingestion and parsing",
      responsible: "platform",
      reason: "register the existing engine-native service account",
    };

    await expect(controller.upsertServiceAccountInventoryV2("crm-parsing", body, "owner@example.test"))
      .resolves.toMatchObject({ status: "stored" });
    expect(upsertServiceAccountInventoryV2).toHaveBeenCalledWith(
      "crm-parsing",
      body,
      "owner@example.test",
    );

    await expect(controller.upsertServiceAccountInventoryV2("", body))
      .rejects.toBeInstanceOf(BadRequestException);
    await expect(controller.upsertServiceAccountInventoryV2("crm-parsing", {
      ...body,
      expected_content_digest: `sha256:v2:${"a".repeat(64)}`,
    })).rejects.toBeInstanceOf(BadRequestException);
  });

  it("maps missing engine accounts to 404 and all ownership/CAS races to 409", async () => {
    const missing = new AdminController({
      upsertServiceAccountInventoryV2: vi.fn().mockRejectedValue(
        new AdminServiceAccountInventoryError("engine_account_missing", "missing"),
      ),
    } as unknown as AdminService);
    const conflict = new AdminController({
      upsertServiceAccountInventoryV2: vi.fn().mockRejectedValue(
        new ServiceAccountInventoryV2Error("version_conflict", "stale"),
      ),
    } as unknown as AdminService);
    const body = {
      expected_source_version: null,
      expected_content_digest: null,
      engine_account_id: "acct_service_crm",
      purpose: "CRM ingestion and parsing",
      responsible: "platform",
      reason: "register the existing engine-native service account",
    };

    await expect(missing.upsertServiceAccountInventoryV2("crm-parsing", body))
      .rejects.toBeInstanceOf(NotFoundException);
    const rejected = conflict.upsertServiceAccountInventoryV2("crm-parsing", body);
    await expect(rejected).rejects.toBeInstanceOf(HttpException);
    await expect(rejected).rejects.toMatchObject({ status: 409 });
  });

  it("maps catalog/rule errors to 400, missing policies to 404, and CAS conflicts to 409", async () => {
    const invalid = new AdminController({
      updateManagedProviderSwitches: vi.fn().mockRejectedValue(
        new PricingPolicyWriteError("rule_outside_catalog", "outside catalog"),
      ),
    } as unknown as AdminService);
    await expect(invalid.updateManagedProviderSwitches({
      expectedGeneration: 1,
      reason: "invalid provider test",
      providers: [{ providerId: "unknown", masterEnabled: true, productEnabled: true, b2cEnabled: true, b2bEnabled: true }],
    })).rejects.toBeInstanceOf(BadRequestException);

    const missing = new AdminController({
      getManagedPricingPolicy: vi.fn().mockRejectedValue(
        new PricingPolicyWriteError("policy_not_found", "missing"),
      ),
    } as unknown as AdminService);
    await expect(missing.getGlobalB2cPricingPolicy()).rejects.toBeInstanceOf(NotFoundException);

    const conflict = new AdminController({
      updateManagedPricingPolicy: vi.fn().mockRejectedValue(
        new PricingPolicyWriteError("version_conflict", "stale"),
      ),
    } as unknown as AdminService);
    const rejected = conflict.updateGlobalB2cPricingPolicy({
      expectedVersion: 1,
      reason: "stale replacement test",
      rules: [rule],
    });
    await expect(rejected).rejects.toBeInstanceOf(HttpException);
    await expect(rejected).rejects.toMatchObject({ status: 409 });
  });
});
