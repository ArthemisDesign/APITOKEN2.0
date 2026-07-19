import type { SalesDatabase } from "./client.js";
import { ReferralCodeCollisionError } from "./auth.js";

export type PartnerApplicationStatus = "pending" | "approved" | "rejected";

export interface PartnerApplication {
  id: string;
  telegramId: string;
  telegramUsername: string | null;
  displayName: string | null;
  telegramPhotoUrl: string | null;
  note: string | null;
  status: PartnerApplicationStatus;
  adminNote: string | null;
  createdPartnerId: string | null;
  createdAt: Date;
  decidedAt: Date | null;
}

export class ApplicationAlreadyDecidedError extends Error {}

interface ApplicationRow {
  id: string;
  telegram_id: string;
  telegram_username: string | null;
  display_name: string | null;
  telegram_photo_url: string | null;
  note: string | null;
  status: PartnerApplicationStatus;
  admin_note: string | null;
  created_partner_id: string | null;
  created_at: Date;
  decided_at: Date | null;
}

const APPLICATION_COLUMNS = `
  id, telegram_id, telegram_username, display_name, telegram_photo_url, note,
  status, admin_note, created_partner_id, created_at, decided_at
`;

function mapApplication(row: ApplicationRow): PartnerApplication {
  return {
    id: row.id,
    telegramId: row.telegram_id,
    telegramUsername: row.telegram_username,
    displayName: row.display_name,
    telegramPhotoUrl: row.telegram_photo_url,
    note: row.note,
    status: row.status,
    adminNote: row.admin_note,
    createdPartnerId: row.created_partner_id,
    createdAt: row.created_at,
    decidedAt: row.decided_at,
  };
}

/** Идемпотентно: существующая pending-заявка того же telegram_id возвращается как есть. */
export async function submitApplication(database: SalesDatabase, input: {
  telegramId: string;
  telegramUsername: string | null;
  displayName: string | null;
  telegramPhotoUrl: string | null;
  note: string | null;
}): Promise<PartnerApplication> {
  const result = await database.pool.query<ApplicationRow>(`
    INSERT INTO partner_applications (telegram_id, telegram_username, display_name, telegram_photo_url, note)
    VALUES ($1, $2, $3, $4, $5)
    ON CONFLICT (telegram_id) WHERE status = 'pending' DO UPDATE
      SET note = COALESCE(EXCLUDED.note, partner_applications.note),
          telegram_username = COALESCE(EXCLUDED.telegram_username, partner_applications.telegram_username)
    RETURNING ${APPLICATION_COLUMNS}
  `, [input.telegramId, input.telegramUsername, input.displayName, input.telegramPhotoUrl, input.note]);
  return mapApplication(result.rows[0]!);
}

export async function getPendingApplicationByTelegramId(
  database: SalesDatabase,
  telegramId: string,
): Promise<PartnerApplication | null> {
  const result = await database.pool.query<ApplicationRow>(`
    SELECT ${APPLICATION_COLUMNS} FROM partner_applications
    WHERE telegram_id = $1 AND status = 'pending'
  `, [telegramId]);
  return result.rows[0] ? mapApplication(result.rows[0]) : null;
}

export async function listApplications(
  database: SalesDatabase,
  status: PartnerApplicationStatus | null,
): Promise<PartnerApplication[]> {
  const result = status
    ? await database.pool.query<ApplicationRow>(`
        SELECT ${APPLICATION_COLUMNS} FROM partner_applications WHERE status = $1 ORDER BY created_at DESC
      `, [status])
    : await database.pool.query<ApplicationRow>(`
        SELECT ${APPLICATION_COLUMNS} FROM partner_applications ORDER BY created_at DESC
      `);
  return result.rows.map(mapApplication);
}

/**
 * Approve: создаёт активного партнёра прямо из заявки (telegram_id уже проверен подписью
 * при подаче) и помечает заявку. Reject: только помечает. Всё в одной транзакции.
 */
export async function decideApplication(database: SalesDatabase, input: {
  applicationId: string;
  action: "approve" | "reject";
  referralCode: string;
  commissionBps: number;
  subCommissionBps: number;
  adminNote: string | null;
  actorId: string;
}): Promise<{ application: PartnerApplication; partnerId: string | null }> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const locked = await client.query<ApplicationRow>(`
      SELECT ${APPLICATION_COLUMNS} FROM partner_applications WHERE id = $1 FOR UPDATE
    `, [input.applicationId]);
    const application = locked.rows[0] ? mapApplication(locked.rows[0]) : null;
    if (!application) throw new ApplicationAlreadyDecidedError("application not found");
    if (application.status !== "pending") throw new ApplicationAlreadyDecidedError("application is already decided");

    let partnerId: string | null = null;
    if (input.action === "approve") {
      const inserted = await client.query<{ id: string }>(`
        INSERT INTO partners (
          telegram_id, telegram_username, telegram_photo_url, display_name, status,
          referral_code, parent_partner_id, commission_bps, sub_commission_bps
        )
        VALUES ($1, $2, $3, $4, 'active', $5, NULL, $6, $7)
        ON CONFLICT DO NOTHING
        RETURNING id
      `, [
        application.telegramId, application.telegramUsername, application.telegramPhotoUrl,
        application.displayName, input.referralCode, input.commissionBps, input.subCommissionBps,
      ]);
      partnerId = inserted.rows[0]?.id ?? null;
      if (!partnerId) {
        // Конфликт: либо партнёр с этим telegram_id уже есть (гонка — линкуем),
        // либо коллизия referral_code (крайне редко — пусть вызывающий повторит с новым кодом).
        const existing = await client.query<{ id: string }>(
          "SELECT id FROM partners WHERE telegram_id = $1",
          [application.telegramId],
        );
        partnerId = existing.rows[0]?.id ?? null;
        if (!partnerId) throw new ReferralCodeCollisionError("referral code collision");
      }
    }
    const updated = await client.query<ApplicationRow>(`
      UPDATE partner_applications
      SET status = $2, admin_note = $3, created_partner_id = $4, decided_at = now()
      WHERE id = $1
      RETURNING ${APPLICATION_COLUMNS}
    `, [application.id, input.action === "approve" ? "approved" : "rejected", input.adminNote, partnerId]);
    await client.query(`
      INSERT INTO sales_audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('admin', $1, $2, 'partner_application', $3, $4::jsonb)
    `, [
      input.actorId,
      input.action === "approve" ? "application.approved" : "application.rejected",
      application.id,
      JSON.stringify({ telegramId: application.telegramId, partnerId }),
    ]);
    await client.query("COMMIT");
    return { application: mapApplication(updated.rows[0]!), partnerId };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}
