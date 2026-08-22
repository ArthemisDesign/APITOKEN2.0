export type AdminPartner = {
  id: string;
  email: string | null;
  telegramUsername: string | null;
  displayName: string | null;
  status: "active" | "suspended" | "pending";
  emailVerified: boolean;
  referralCode: string;
  commissionBps: number;
  subCommissionBps: number;
  teamOverrideMaxBps: number;
  parentOverrideBps: number | null;
  b2bEnabled: boolean;
  b2bMaxDiscountBps: number;
  teamInvitesEnabled: boolean;
  b2bCanDelegate: boolean;
  b2bGrantSourcePartnerId: string | null;
  parentPartnerId: string | null;
  parentEmail: string | null;
  parentTelegramUsername: string | null;
  referredUsers: number;
  teamSize: number;
  earnedNano: string;
  adjustmentNano: string;
  netNano: string;
  debtNano: string;
  payableNano: string;
  paidNano: string;
  createdAt: string;
};

export type PartnerRequestType = "b2b_conversion" | "b2b_pricing" | "commission_change";
export type PartnerRequestStatus = "pending" | "approved" | "rejected" | "applied" | "apply_failed";
export type ProviderId = "anthropic" | "openai" | "google" | "kimi" | "glm";
export type AdminPartnerRequest = {
  id: string;
  requestType: PartnerRequestType;
  status: PartnerRequestStatus;
  requesterPartnerId: string;
  requesterEmail: string | null;
  requesterDisplayName: string | null;
  subjectPartnerId: string | null;
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
    id: string;
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

export type PartnerApplication = {
  id: string;
  telegramUsername: string | null;
  displayName: string | null;
  note: string | null;
  status: "pending" | "approved" | "rejected";
  adminNote: string | null;
  createdAt: string;
  decidedAt: string | null;
};

export type RootInvite = {
  code: string;
  inviteUrl: string;
  telegramUsername: string | null;
  commissionBps: number | null;
  subCommissionBps: number | null;
  teamOverrideMaxBps: number;
  teamInvitesEnabled: boolean;
  b2bEnabled: boolean;
  b2bMaxDiscountBps: number;
  b2bCanDelegate: boolean;
  expiresAt: string | null;
  consumedAt: string | null;
};

export type PartnerAnalyticsDetail = {
  id: string;
  email: string | null;
  telegramUsername: string | null;
  displayName: string | null;
  status: "active" | "suspended" | "pending";
  referralCode: string;
  parentId: string | null;
  parentLabel: string | null;
  commissionBps: number;
  subCommissionBps: number;
  teamOverrideMaxBps?: number;
  teamInvitesEnabled?: boolean;
  b2bEnabled: boolean;
  b2bMaxDiscountBps: number;
  b2bCanDelegate?: boolean;
  depositsTotalNano: string;
  deposits30dNano: string;
  referredUsers: number;
  convertedUsers: number;
  spendTotalNano: string;
  spend30dNano: string;
  earnedTotalNano: string;
  earned30dNano: string;
  adjustmentTotalNano: string;
  adjustment30dNano: string;
  netTotalNano: string;
  net30dNano: string;
  paidNano: string;
  unpaidNano: string;
  debtNano: string;
  payableNano: string;
  teamSize: number;
  lastSeenAt: string | null;
  lastReferralAt: string | null;
  lastDepositAt: string | null;
  createdAt: string;
};

export type PartnerDetailBundle = {
  partner: PartnerAnalyticsDetail;
  daily: Array<{ date: string; spendNano: string; earnedNano: string; adjustmentNano: string; netNano: string }>;
  team: Array<{
    id: string; email: string | null; telegramUsername: string | null; displayName: string | null;
    commissionBps: number; overrideBps?: number; teamOverrideMaxBps?: number; referredUsers: number;
    myOverrideNetNano: string; status?: string;
  }>;
  payouts: Array<{ id: string; amountNano: string; status: string; requestedAt: string; paidAt: string | null; adminNote?: string | null }>;
  referrals: Array<{
    userMask: string; userRef: string; email: string | null; attributedAt: string;
    spendNano: string; earnedNano: string; adjustmentNano: string; netNano: string;
    customerType: "b2c" | "b2b" | null; discountPercent: number | null;
  }>;
};

export type PartnerActivity = {
  type: string;
  at: string;
  amountNano: string | null;
  label: string;
  email?: string | null;
  userMask?: string | null;
  meta: Record<string, unknown>;
};

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
