// Типы payload'ов бэкенд-эндпоинтов админки. Все поля опциональны: панель
// деградирует молча (catch → null, пропуски → "—"), как admin-panel.js.
// Деньги в nanoUSD — строки (*Nano); легаси-поля коммерции в долларах — number
// (*_usd, balance_usd), только для отображения через money().

// GET /admin/dashboard (коммерция)
export interface CommerceDashboard {
  generated_at?: string;
  users?: {
    total?: number;
    active?: number;
    disabled?: number;
    registered_oauth?: number;
    oauth_only?: number;
    hybrid?: number;
    google?: number;
    github?: number;
    registered_password?: number;
    password_only?: number;
    registered_30d?: number;
    registered_24h?: number;
    active_7d?: number;
    totp?: number;
    verified?: number;
  };
  topups?: {
    paid_count?: number;
    paid_users?: number;
    paid_usd?: number;
    paid_30d_usd?: number;
    paid_30d_count?: number;
    manual_count?: number;
    manual_usd?: number;
    manual_30d_count?: number;
    manual_30d_usd?: number;
    pending_checkouts?: number;
    failed_30d?: number;
    refunded_count?: number;
    refunded_usd?: number;
  };
  platform?: {
    engine_error?: number;
    active_sessions?: number;
    active_api_keys?: number;
    total_api_keys?: number;
    b2c_users?: number;
    b2b_users?: number;
    engine_active?: number;
    engine_pending?: number;
    engine_disabled?: number;
  };
}

// GET /overview (движок)
export interface EngineAccount {
  handle?: string;
  status?: string;
  balance_usd?: number;
  [key: string]: unknown;
}

export interface EngineOverview {
  accounts?: EngineAccount[];
  [key: string]: unknown;
}

// GET /partner-admin/overview (партнёрка)
export interface PartnerOverview {
  partners?: number;
  activePartners?: number;
  referredUsers?: number;
  totalCommissionsNano?: string;
  totalAdjustmentsNano?: string;
  totalNetCommissionsNano?: string;
  totalDebtNano?: string;
  totalPayableNano?: string;
  pendingPayoutsNano?: string;
  paidPayoutsNano?: string;
  [key: string]: unknown;
}

// GET /admin/pipeline-health (коммерция)
export interface PipelineHealth {
  verdict?: string;
  verdict_reasons?: string[];
  [key: string]: unknown;
}

// GET /settlement-health (движок)
export interface SettlementHealth {
  outbox?: {
    failed_24h?: number;
    backlog?: number;
    [key: string]: unknown;
  };
  [key: string]: unknown;
}
