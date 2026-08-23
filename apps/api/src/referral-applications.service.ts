import { Inject, Injectable, NotFoundException, UnprocessableEntityException } from "@nestjs/common";
import {
  decideReferralApplication,
  findLatestReferralApplication,
  findReferralApplication,
  listReferralApplications,
  submitReferralApplication,
  type Database,
  type ReferralApplication,
} from "@claude-api/db";
import { DATABASE } from "./infrastructure.module.js";
import { ReferralService, type PartnerAuthorityInput } from "./referral.service.js";

/** Standard terms an approved application starts on; an admin can widen them afterwards. */
export const DEFAULT_APPLICATION_TERMS: { commissionBps: number; authority: PartnerAuthorityInput } = {
  commissionBps: 1_000,
  authority: {
    teamOverrideMaxBps: 2_000,
    teamInvitesEnabled: true,
    b2bEnabled: false,
    b2bMaxDiscountBps: 0,
    b2bCanDelegate: false,
  },
};

@Injectable()
export class ReferralApplicationService {
  constructor(
    @Inject(DATABASE) private readonly database: Database,
    private readonly referral: ReferralService,
  ) {}

  /** The applicant's own view: the latest application, or null when they never applied. */
  async mine(userId: string): Promise<{ application: ReferralApplication | null }> {
    return { application: await findLatestReferralApplication(this.database, userId) };
  }

  async submit(userId: string, message: string): Promise<{ application: ReferralApplication }> {
    return { application: await submitReferralApplication(this.database, { userId, message }) };
  }

  async list(query: { status?: "pending" | "approved" | "rejected" | undefined; limit?: number | undefined }): Promise<{ items: ReferralApplication[] }> {
    return { items: await listReferralApplications(this.database, query) };
  }

  /**
   * Approve or reject. Approval enables partner access through the same onboarding path an
   * administrator uses by hand, so an approved application and a manual onboarding are the same
   * operation — the decision is only recorded after that succeeds.
   */
  async decide(input: {
    id: string;
    action: "approve" | "reject";
    note: string;
    actor: string;
    commissionBps?: number | undefined;
    authority?: PartnerAuthorityInput | undefined;
  }): Promise<{ application: ReferralApplication }> {
    const application = await findReferralApplication(this.database, input.id);
    if (!application) throw new NotFoundException("application not found");
    if (application.status !== "pending") throw new UnprocessableEntityException("application already decided");

    if (input.action === "approve") {
      await this.referral.onboardByEmail({
        email: application.email,
        commissionBps: input.commissionBps ?? DEFAULT_APPLICATION_TERMS.commissionBps,
        authority: input.authority ?? DEFAULT_APPLICATION_TERMS.authority,
        actor: input.actor,
      });
    }

    const decided = await decideReferralApplication(this.database, {
      id: input.id,
      status: input.action === "approve" ? "approved" : "rejected",
      reviewerActor: input.actor,
      reviewerNote: input.note,
    });
    if (!decided) throw new UnprocessableEntityException("application already decided");
    return { application: decided };
  }
}
