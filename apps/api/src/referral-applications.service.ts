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
import { DevbotPartnerNotifier } from "./devbot-partner.notifier.js";
import { ReferralService, type PartnerAuthorityInput } from "./referral.service.js";

/** Standard terms an approved application starts on; an admin can widen them afterwards. */
export const DEFAULT_APPLICATION_TERMS: { commissionBps: number; authority: PartnerAuthorityInput } = {
  commissionBps: 1_000,
  authority: {
    // Building a Team and setting B2B terms are ordinary capabilities; only their ceilings are set.
    teamOverrideMaxBps: 2_000,
    teamInvitesEnabled: true,
    b2bEnabled: true,
    b2bMaxDiscountBps: 5_000,
    b2bCanDelegate: true,
  },
};

@Injectable()
export class ReferralApplicationService {
  constructor(
    @Inject(DATABASE) private readonly database: Database,
    private readonly referral: ReferralService,
    private readonly devbot: DevbotPartnerNotifier,
  ) {}

  /** The applicant's own view: the latest application, or null when they never applied. */
  async mine(userId: string): Promise<{ application: ReferralApplication | null }> {
    return { application: await findLatestReferralApplication(this.database, userId) };
  }

  async submit(userId: string, message: string): Promise<{ application: ReferralApplication }> {
    const previous = await findLatestReferralApplication(this.database, userId);
    const application = await submitReferralApplication(this.database, { userId, message });
    this.devbot.notify({
      event: previous?.status === "pending" && previous.id === application.id ? "updated" : "submitted",
      ...webhookFields(application),
    });
    return { application };
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
    this.devbot.notify({ event: "decided", ...webhookFields(decided) });
    return { application: decided };
  }
}

function webhookFields(application: ReferralApplication) {
  return {
    id: application.id,
    email: application.email,
    status: application.status,
    message: application.message,
    createdAt: application.createdAt,
    reviewerActor: application.reviewerActor,
    reviewerNote: application.reviewerNote,
  };
}
