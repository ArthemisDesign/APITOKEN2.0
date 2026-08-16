import { Inject, Injectable, ServiceUnavailableException } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { listCrmReferralRegistrations, type Database } from "@claude-api/db";
import { type EngineClient } from "@claude-api/engine-client";
import { z } from "zod";
import type { Environment } from "./config.js";
import { DATABASE, ENGINE_CLIENT } from "./infrastructure.module.js";

const SALES_TIMEOUT_MS = 4_000;
const CRM_REFERRAL_SOURCE = "crm";

const salesAliasSchema = z.object({
  source: z.literal(CRM_REFERRAL_SOURCE),
  externalRef: z.string().uuid(),
  code: z.string().regex(/^r_[a-z0-9_-]{20,64}$/),
  partnerId: z.string().uuid(),
  createdAt: z.string().datetime({ offset: true }),
}).strict();

export interface CrmReferralLinkView {
  schemaVersion: 1;
  externalRef: string;
  referralAlias: string;
  destinationUrl: string;
  createdAt: string;
}

export interface CrmReferralProfileView {
  schemaVersion: 1;
  externalRef: string;
  referralAlias: string;
  attributionStatus: "none" | "unique" | "ambiguous";
  registrations: Array<{
    candidateId: string;
    binding: "link_attributed";
    email: string;
    emailVerified: boolean;
    registeredAt: string;
    customerStatus: "active" | "disabled";
    engineStatus: "active" | "disabled" | "pending" | "error" | null;
    customerType: "b2c" | "b2b" | null;
    pricing: {
      defaultMultiplierBp: number | null;
      defaultDiscountBps: number | null;
      defaultState: "live" | "saved" | "unavailable";
      providerOverrides: Array<{
        providerId: string;
        multiplierBp: number;
        discountBps: number;
      }>;
    };
    money: {
      currency: "USD";
      paidTopupNano: string;
      refundedNano: string;
      usageSpentNano: string;
      customerFundedSpentNano: string;
      balanceNano: string | null;
      liveState: "complete" | "unavailable" | "not_provisioned";
    };
  }>;
  asOf: string;
}

@Injectable()
export class CrmBridgeService {
  constructor(
    @Inject(DATABASE) private readonly database: Database,
    @Inject(ENGINE_CLIENT) private readonly engine: EngineClient,
    private readonly config: ConfigService<Environment, true>,
  ) {}

  async ensureReferralLink(externalRef: string): Promise<CrmReferralLinkView> {
    const alias = await this.ensureSalesAlias(externalRef);
    const destination = new URL(this.config.get("PUBLIC_APP_BASE_URL", { infer: true }));
    destination.pathname = "/";
    destination.search = "";
    destination.hash = "";
    destination.searchParams.set("ref", alias.code);
    destination.searchParams.set("utm_source", "crm");
    destination.searchParams.set("utm_medium", "direct_sales");
    destination.searchParams.set("utm_campaign", "crm-referral");
    destination.searchParams.set("utm_content", alias.code);
    return {
      schemaVersion: 1,
      externalRef,
      referralAlias: alias.code,
      destinationUrl: destination.toString(),
      createdAt: alias.createdAt,
    };
  }

  async referralProfile(externalRef: string): Promise<CrmReferralProfileView> {
    const alias = await this.ensureSalesAlias(externalRef);
    const rows = await listCrmReferralRegistrations(this.database, alias.code);
    const engineIds = rows.flatMap((row) => row.engineAccountId ? [row.engineAccountId] : []);
    let liveAccounts: Awaited<ReturnType<EngineClient["getAccounts"]>> = [];
    let engineAvailable = true;
    if (engineIds.length > 0) {
      try {
        liveAccounts = await this.engine.getAccounts(engineIds);
      } catch {
        engineAvailable = false;
      }
    }
    const liveById = new Map(liveAccounts.map((account) => [account.account, account]));

    return {
      schemaVersion: 1,
      externalRef,
      referralAlias: alias.code,
      attributionStatus: rows.length === 0 ? "none" : rows.length === 1 ? "unique" : "ambiguous",
      registrations: rows.map((row) => {
        const live = row.engineAccountId ? liveById.get(row.engineAccountId) : undefined;
        const defaultMultiplierBp = live?.mult_bp ?? row.defaultMultiplierBp;
        const liveState = row.engineAccountId === null
          ? "not_provisioned" as const
          : engineAvailable && live
            ? "complete" as const
            : "unavailable" as const;
        return {
          candidateId: row.candidateId,
          binding: "link_attributed" as const,
          email: row.email,
          emailVerified: row.emailVerified,
          registeredAt: row.registeredAt.toISOString(),
          customerStatus: row.customerStatus,
          engineStatus: live?.status ?? (row.engineAccountId === null ? row.projectedEngineStatus : null),
          customerType: row.customerType,
          pricing: {
            defaultMultiplierBp,
            defaultDiscountBps: defaultMultiplierBp === null ? null : 10_000 - defaultMultiplierBp,
            defaultState: live ? "live" as const : defaultMultiplierBp === null ? "unavailable" as const : "saved" as const,
            providerOverrides: row.providerOverrides.map((override) => ({
              ...override,
              discountBps: 10_000 - override.multiplierBp,
            })),
          },
          money: {
            currency: "USD" as const,
            paidTopupNano: row.paidTopupNano.toString(),
            refundedNano: row.refundedNano.toString(),
            usageSpentNano: row.usageSpentNano.toString(),
            customerFundedSpentNano: row.customerFundedSpentNano.toString(),
            balanceNano: live?.balance_nano ?? null,
            liveState,
          },
        };
      }),
      asOf: new Date().toISOString(),
    };
  }

  private async ensureSalesAlias(externalRef: string): Promise<z.infer<typeof salesAliasSchema>> {
    const base = this.config.get("SALES_API_URL", { infer: true });
    const key = this.config.get("SALES_CONTROL_KEY", { infer: true });
    const partnerCode = this.config.get("CRM_REFERRAL_PARTNER_CODE", { infer: true });
    if (!base || !key || !partnerCode) throw new ServiceUnavailableException("CRM referral bridge is disabled");

    try {
      const response = await fetch(new URL("/v1/internal/partners/external-referral-alias", base), {
        method: "POST",
        headers: { "content-type": "application/json", "x-api-key": key },
        body: JSON.stringify({
          source: CRM_REFERRAL_SOURCE,
          externalRef,
          partnerCode: partnerCode.toLowerCase(),
        }),
        signal: AbortSignal.timeout(SALES_TIMEOUT_MS),
      });
      if (!response.ok) {
        throw new Error(`Sales alias producer returned ${response.status}`);
      }
      const alias = salesAliasSchema.parse(await response.json());
      if (alias.externalRef !== externalRef) throw new Error("Sales alias producer changed externalRef");
      return alias;
    } catch {
      throw new ServiceUnavailableException("CRM referral alias is temporarily unavailable");
    }
  }
}
