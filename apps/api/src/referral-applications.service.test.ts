import { NotFoundException, UnprocessableEntityException } from "@nestjs/common";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ReferralApplication } from "@claude-api/db";
import { DEFAULT_APPLICATION_TERMS, ReferralApplicationService } from "./referral-applications.service.js";

const APPLICATION: ReferralApplication = {
  id: "6a3a0f4b-2f24-4a2e-a0a5-2f1ad2d7f4b1",
  userId: "0b5f9b52-1a44-4f0b-8f0a-0f5a5c2a1a11",
  email: "partner@example.com",
  status: "pending",
  message: "I run an agency with three AI products.",
  reviewerActor: null,
  reviewerNote: null,
  decidedAt: null,
  createdAt: "2026-08-23T09:00:00.000Z",
};

/** The service reads its queries from @claude-api/db, so the module is faked wholesale. */
vi.mock("@claude-api/db", () => ({
  findReferralApplication: vi.fn(),
  findLatestReferralApplication: vi.fn(),
  listReferralApplications: vi.fn(),
  submitReferralApplication: vi.fn(),
  decideReferralApplication: vi.fn(),
}));

const db = await import("@claude-api/db") as unknown as {
  findReferralApplication: ReturnType<typeof vi.fn>;
  findLatestReferralApplication: ReturnType<typeof vi.fn>;
  listReferralApplications: ReturnType<typeof vi.fn>;
  submitReferralApplication: ReturnType<typeof vi.fn>;
  decideReferralApplication: ReturnType<typeof vi.fn>;
};

function service(onboard = vi.fn().mockResolvedValue({ ok: true })) {
  const referral = { onboardByEmail: onboard } as never;
  return { service: new ReferralApplicationService({} as never, referral), onboard };
}

describe("Partner access applications", () => {
  beforeEach(() => { vi.clearAllMocks(); });

  it("approves by running the same onboarding an administrator runs by hand", async () => {
    db.findReferralApplication.mockResolvedValue(APPLICATION);
    db.decideReferralApplication.mockResolvedValue({ ...APPLICATION, status: "approved", reviewerActor: "ops", reviewerNote: "Known agency." });
    const { service: subject, onboard } = service();

    const result = await subject.decide({ id: APPLICATION.id, action: "approve", note: "Known agency.", actor: "ops" });

    expect(onboard).toHaveBeenCalledWith({
      email: APPLICATION.email,
      commissionBps: DEFAULT_APPLICATION_TERMS.commissionBps,
      authority: DEFAULT_APPLICATION_TERMS.authority,
      actor: "ops",
    });
    expect(result.application.status).toBe("approved");
  });

  it("records a rejection without touching partner onboarding", async () => {
    db.findReferralApplication.mockResolvedValue(APPLICATION);
    db.decideReferralApplication.mockResolvedValue({ ...APPLICATION, status: "rejected", reviewerActor: "ops", reviewerNote: "No traffic yet." });
    const { service: subject, onboard } = service();

    const result = await subject.decide({ id: APPLICATION.id, action: "reject", note: "No traffic yet.", actor: "ops" });

    expect(onboard).not.toHaveBeenCalled();
    expect(result.application.status).toBe("rejected");
  });

  it("refuses a second decision on an application that is already decided", async () => {
    db.findReferralApplication.mockResolvedValue({ ...APPLICATION, status: "approved" });
    const { service: subject, onboard } = service();

    await expect(subject.decide({ id: APPLICATION.id, action: "approve", note: "again", actor: "ops" }))
      .rejects.toBeInstanceOf(UnprocessableEntityException);
    expect(onboard).not.toHaveBeenCalled();
  });

  it("does not record a decision when onboarding fails", async () => {
    db.findReferralApplication.mockResolvedValue(APPLICATION);
    const onboard = vi.fn().mockRejectedValue(new Error("sales unavailable"));
    const { service: subject } = service(onboard);

    await expect(subject.decide({ id: APPLICATION.id, action: "approve", note: "ok", actor: "ops" })).rejects.toThrow("sales unavailable");
    expect(db.decideReferralApplication).not.toHaveBeenCalled();
  });

  it("reports a missing application instead of onboarding an unknown account", async () => {
    db.findReferralApplication.mockResolvedValue(null);
    const { service: subject, onboard } = service();

    await expect(subject.decide({ id: APPLICATION.id, action: "approve", note: "ok", actor: "ops" }))
      .rejects.toBeInstanceOf(NotFoundException);
    expect(onboard).not.toHaveBeenCalled();
  });
});
