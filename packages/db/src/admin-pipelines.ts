import type { Database } from "./client.js";

// Read-only сводка здоровья денежных пайплайнов для админ-панели
// (admin.apitoken.sale → GET /admin/pipeline-health). Источник — commerce PostgreSQL
// (engine_credits, webhook_events, email_outbox, engine_pricing_jobs): те очереди, через
// которые деньги клиента доезжают до engine-баланса и обратно. Ничего не пишется. Все
// денежные суммы отдаются строками nano-USD (без JS number и float), агрегация — на стороне
// БД (GROUP BY/FILTER), таблицы в память не выгружаются.
//
// Безопасность: webhook_events.payload и email_outbox.payload НЕ читаются и не отдаются —
// payload вебхука содержит сырой body провайдера (подписи, order id, PII плательщика), а
// payload письма — персональные данные получателя. Наружу идут только технические поля
// (provider/event_type/template/attempts/timestamps) и last_error, обрезанный до 200
// символов на стороне БД.

export interface AdminEngineCreditHealth {
  countsByStatus: {
    pending: number;
    processing: number;
    retry: number;
    confirmed: number;
    dead: number;
  };
  /** Кредиты в status='retry' с attempts >= 3 (перепробовали уже много раз). */
  retryHighAttempts: number;
  /** created_at самого старого кредита не в 'confirmed' (деньги в пути/застряли). */
  oldestUnconfirmedCreatedAt: Date | null;
  /** Его возраст в секундах (целое); null, если неподтверждённых нет. */
  oldestUnconfirmedAgeSeconds: number | null;
  /** Сумма amount_nano всех кредитов не в 'confirmed' — деньги клиентов в пути/застрявшие. */
  stuckNano: string;
}

export interface AdminWebhookFailureRow {
  provider: string;
  eventType: string;
  attempts: number;
  receivedAt: Date;
  lastError: string | null;
}

export interface AdminWebhookHealth {
  failedTotal: number;
  failed24h: number;
  recentFailures: AdminWebhookFailureRow[];
}

export interface AdminEmailFailureRow {
  template: string;
  attempts: number;
  lastError: string | null;
}

export interface AdminEmailOutboxHealth {
  failedTotal: number;
  recentFailures: AdminEmailFailureRow[];
}

export interface AdminPricingJobErrorRow {
  userId: string;
  engineAccountId: string;
  reason: string;
  status: string;
  attempts: number;
  lastError: string | null;
}

export interface AdminPricingJobHealth {
  countsByStatus: {
    pending: number;
    processing: number;
    retry: number;
    confirmed: number;
  };
  /** Неподтверждённые джобы с last_error (последние ≤5 по updated_at). */
  recentErrors: AdminPricingJobErrorRow[];
  oldestUnconfirmedCreatedAt: Date | null;
  oldestUnconfirmedAgeSeconds: number | null;
}

/**
 * engine_credits: counts по всем статусам (стабильная форма даже на пустой таблице — нули),
 * dead, retry с attempts >= 3, возраст самого старого не-confirmed (по created_at) и сумма
 * nano, зависшая в не-confirmed. Одна строка агрегатов, FILTER вместо нескольких запросов.
 */
export async function getAdminEngineCreditHealth(
  database: Database,
): Promise<AdminEngineCreditHealth> {
  const result = await database.pool.query<{
    pending: string;
    processing: string;
    retry: string;
    confirmed: string;
    dead: string;
    retry_high_attempts: string;
    oldest_unconfirmed_created_at: Date | null;
    oldest_unconfirmed_age_seconds: string | null;
    stuck_nano: string;
  }>(`
    /* admin-pipelines:engine-credits */
    SELECT
      count(*) FILTER (WHERE status = 'pending')::text AS pending,
      count(*) FILTER (WHERE status = 'processing')::text AS processing,
      count(*) FILTER (WHERE status = 'retry')::text AS retry,
      count(*) FILTER (WHERE status = 'confirmed')::text AS confirmed,
      count(*) FILTER (WHERE status = 'dead')::text AS dead,
      count(*) FILTER (WHERE status = 'retry' AND attempts >= 3)::text AS retry_high_attempts,
      min(created_at) FILTER (WHERE status <> 'confirmed') AS oldest_unconfirmed_created_at,
      round(EXTRACT(EPOCH FROM (now() - min(created_at) FILTER (WHERE status <> 'confirmed'))))::text
        AS oldest_unconfirmed_age_seconds,
      COALESCE(sum(amount_nano) FILTER (WHERE status <> 'confirmed'), 0)::text AS stuck_nano
    FROM engine_credits
  `);
  const row = result.rows[0]!;
  return {
    countsByStatus: {
      pending: Number(row.pending),
      processing: Number(row.processing),
      retry: Number(row.retry),
      confirmed: Number(row.confirmed),
      dead: Number(row.dead),
    },
    retryHighAttempts: Number(row.retry_high_attempts),
    oldestUnconfirmedCreatedAt: row.oldest_unconfirmed_created_at,
    oldestUnconfirmedAgeSeconds: row.oldest_unconfirmed_age_seconds === null
      ? null
      : Number(row.oldest_unconfirmed_age_seconds),
    stuckNano: row.stuck_nano,
  };
}

/**
 * webhook_events: failed всего и за последние 24 часа (по received_at) + последние ≤10
 * failed (provider, event_type, attempts, received_at, last_error ≤200 символов). Поле
 * payload намеренно не читается: там сырой body провайдера с подписями и PII плательщика.
 */
export async function getAdminWebhookHealth(database: Database): Promise<AdminWebhookHealth> {
  const [counts, failures] = await Promise.all([
    database.pool.query<{ failed_total: string; failed_24h: string }>(`
      /* admin-pipelines:webhook-failed-counts */
      SELECT
        count(*) FILTER (WHERE status = 'failed')::text AS failed_total,
        count(*) FILTER (
          WHERE status = 'failed' AND received_at >= now() - interval '24 hours'
        )::text AS failed_24h
      FROM webhook_events
    `),
    database.pool.query<{
      provider: string; event_type: string; attempts: number;
      received_at: Date; last_error: string | null;
    }>(`
      /* admin-pipelines:webhook-recent-failed */
      SELECT provider, event_type, attempts, received_at, left(last_error, 200) AS last_error
      FROM webhook_events
      WHERE status = 'failed'
      ORDER BY received_at DESC, id
      LIMIT 10
    `),
  ]);
  return {
    failedTotal: Number(counts.rows[0]!.failed_total),
    failed24h: Number(counts.rows[0]!.failed_24h),
    recentFailures: failures.rows.map((row) => ({
      provider: row.provider,
      eventType: row.event_type,
      attempts: row.attempts,
      receivedAt: row.received_at,
      lastError: row.last_error,
    })),
  };
}

/**
 * email_outbox: терминально failed всего + последние ≤5 failed (template, attempts,
 * last_error ≤200 символов). payload письма (ПДн получателя) не читается.
 */
export async function getAdminEmailOutboxHealth(
  database: Database,
): Promise<AdminEmailOutboxHealth> {
  const [counts, failures] = await Promise.all([
    database.pool.query<{ failed_total: string }>(`
      /* admin-pipelines:email-failed-count */
      SELECT count(*)::text AS failed_total
      FROM email_outbox
      WHERE status = 'failed'
    `),
    database.pool.query<{ template: string; attempts: number; last_error: string | null }>(`
      /* admin-pipelines:email-recent-failed */
      SELECT template, attempts, left(last_error, 200) AS last_error
      FROM email_outbox
      WHERE status = 'failed'
      ORDER BY updated_at DESC, id
      LIMIT 5
    `),
  ]);
  return {
    failedTotal: Number(counts.rows[0]!.failed_total),
    recentFailures: failures.rows.map((row) => ({
      template: row.template,
      attempts: row.attempts,
      lastError: row.last_error,
    })),
  };
}

/**
 * engine_pricing_jobs: counts по статусам (enum pricing_job_status: pending/processing/
 * retry/confirmed — терминального error/dead у джоб нет, поэтому «ошибочные» — это
 * не-confirmed с last_error), последние ≤5 таких ошибок и возраст самого старого
 * не-confirmed (по created_at).
 */
export async function getAdminPricingJobHealth(
  database: Database,
): Promise<AdminPricingJobHealth> {
  const [counts, errors] = await Promise.all([
    database.pool.query<{
      pending: string;
      processing: string;
      retry: string;
      confirmed: string;
      oldest_unconfirmed_created_at: Date | null;
      oldest_unconfirmed_age_seconds: string | null;
    }>(`
      /* admin-pipelines:pricing-jobs */
      SELECT
        count(*) FILTER (WHERE status = 'pending')::text AS pending,
        count(*) FILTER (WHERE status = 'processing')::text AS processing,
        count(*) FILTER (WHERE status = 'retry')::text AS retry,
        count(*) FILTER (WHERE status = 'confirmed')::text AS confirmed,
        min(created_at) FILTER (WHERE status <> 'confirmed') AS oldest_unconfirmed_created_at,
        round(EXTRACT(EPOCH FROM (now() - min(created_at) FILTER (WHERE status <> 'confirmed'))))::text
          AS oldest_unconfirmed_age_seconds
      FROM engine_pricing_jobs
    `),
    database.pool.query<{
      user_id: string; engine_account_id: string; reason: string; status: string;
      attempts: number; last_error: string | null;
    }>(`
      /* admin-pipelines:pricing-jobs-recent-errors */
      SELECT user_id, engine_account_id, reason, status::text AS status, attempts,
             left(last_error, 200) AS last_error
      FROM engine_pricing_jobs
      WHERE status <> 'confirmed' AND last_error IS NOT NULL
      ORDER BY updated_at DESC, id
      LIMIT 5
    `),
  ]);
  const row = counts.rows[0]!;
  return {
    countsByStatus: {
      pending: Number(row.pending),
      processing: Number(row.processing),
      retry: Number(row.retry),
      confirmed: Number(row.confirmed),
    },
    recentErrors: errors.rows.map((error) => ({
      userId: error.user_id,
      engineAccountId: error.engine_account_id,
      reason: error.reason,
      status: error.status,
      attempts: error.attempts,
      lastError: error.last_error,
    })),
    oldestUnconfirmedCreatedAt: row.oldest_unconfirmed_created_at,
    oldestUnconfirmedAgeSeconds: row.oldest_unconfirmed_age_seconds === null
      ? null
      : Number(row.oldest_unconfirmed_age_seconds),
  };
}
