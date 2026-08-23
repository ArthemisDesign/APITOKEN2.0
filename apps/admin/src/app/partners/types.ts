export type PartnerStatus = "active" | "suspended" | "pending";

/** Commerce-enriched projection. Internal Commerce and Sales identifiers never reach the browser. */
export type AdminPartner = {
  email: string | null;
  accountStatus: "active" | "disabled" | null;
  programEnabled: boolean;
  programStartedAt: string | null;
  status: PartnerStatus;
  referralCode: string;
  commissionBps: number;
  teamOverrideMaxBps: number;
  teamShareBps: number | null;
  parentEmail: string | null;
  referredUsers: number;
  teamSize: number;
  earnedNano: string;
  adjustmentNano: string;
  netNano: string;
  debtNano: string;
  payableNano: string;
  paidNano: string;
  b2bEnabled: boolean;
  b2bMaxDiscountBps: number;
  teamInvitesEnabled: boolean;
  b2bCanDelegate: boolean;
  createdAt: string;
};

export type PartnerRequestType = "b2b_conversion" | "b2b_pricing" | "commission_change";
export type PartnerRequestStatus = "pending" | "approved" | "rejected" | "applied" | "apply_failed";
export type ProviderId = "anthropic" | "openai" | "google" | "kimi" | "glm";

export type AdminPartnerRequest = {
  id: string;
  requestType: PartnerRequestType;
  status: PartnerRequestStatus;
  requesterEmail: string | null;
  customerEmail: string | null;
  reason: string;
  stateSnapshot: Record<string, unknown>;
  requestedCommissionBps: number | null;
  requestedDiscountBps: number | null;
  approvedCommissionBps: number | null;
  approvedDiscountBps: number | null;
  reviewerActor: string | null;
  reviewerNote: string | null;
  reviewedAt: string | null;
  appliedAt: string | null;
  applyAttempts: number;
  lastApplyError: string | null;
  version: number;
  providerTerms: Array<{
    providerId: ProviderId;
    requestedDiscountBps: number | null;
    approvedDiscountBps: number | null;
    decided: boolean;
  }>;
  effect: null | {
    status: "pending" | "processing" | "applied" | "failed";
    attempts: number;
    nextAttemptAt: string | null;
    terminal: boolean;
    appliedAt: string | null;
    lastError: string | null;
  };
  createdAt: string;
  updatedAt: string;
};

export type PartnerRequestsPage = { items: AdminPartnerRequest[]; nextCursor: string | null };

export type AdminPartnerPayout = {
  id: string;
  email: string | null;
  amountNano: string;
  status: "requested" | "approved" | "paid" | "rejected";
  method: string;
  details: unknown;
  requestedAt: string;
  decidedAt: string | null;
  paidAt: string | null;
  adminNote: string | null;
  txHash: string | null;
  chainStatus: string | null;
};

// The on-chain executor is still a separate, fenced Sales surface. It stays isolated from
// Commerce identity onboarding until its producer contract is extended.
export type PayoutBatch = {
  id: string;
  status: "preparing" | "prepared" | "sending" | "sent" | "failed" | "canceled";
  hotWalletAddress: string | null;
  totalNano: string;
  recipientCount: number;
  gasPriceGwei: string | number | null;
  minNano: string;
  earnedBefore: string | null;
  note: string | null;
  createdBy: string;
  error: string | null;
  createdAt: string;
  preparedAt: string | null;
  sentAt: string | null;
  completedAt: string | null;
};

export type PayoutRow = {
  id: string;
  partnerId: string;
  partner: string;
  amountNano: string;
  status: string;
  walletAddress: string | null;
  txHash: string | null;
  chainStatus: string | null;
  chainError: string | null;
  paidAt: string | null;
};

export type PayoutEngineState = {
  configured: boolean;
  window: { open: boolean; enforced: boolean; opensAt: string | null; closesAt: string | null };
};

export type PayoutReport = {
  batch: PayoutBatch;
  rows: PayoutRow[];
  window: PayoutEngineState["window"];
  chain: {
    configured: boolean;
    hotWalletAddress: string | null;
    currentHotWalletAddress: string | null;
    configurationMatchesBatch: boolean | null;
    requiredUsdtNano: string;
    requiredBnbWei: string | null;
    usdtBalanceNano: string | null;
    bnbBalanceWei: string | null;
    sufficientUsdt: boolean | null;
    sufficientBnb: boolean | null;
    gasPriceGwei: string;
  };
  invalidAddresses: Array<{ partnerId: string; walletAddress: string; reason: string }>;
  accounting: null | {
    ready: boolean; reasons: string[];
    usageCursor: string; usageSourceHead: string;
    fundingLotCursor: string; fundingLotSourceHead: string;
    paymentReversalCursor: string; paymentReversalSourceHead: string;
    incompleteUsageCount: string; missingCommissionSliceCount: string; incompleteReversalCount: string;
    reversalCount: string; adjustmentCount: string; adjustmentNano: string;
  };
};

export type PartnerApplicationStatus = "pending" | "approved" | "rejected";

/** An ordinary Commerce account asking for partner access. Identity is the account email. */
export type AdminPartnerApplication = {
  id: string;
  email: string;
  status: PartnerApplicationStatus;
  message: string;
  reviewerActor: string | null;
  reviewerNote: string | null;
  decidedAt: string | null;
  createdAt: string;
};

export type PartnerApplicationsPage = { items: AdminPartnerApplication[] };
