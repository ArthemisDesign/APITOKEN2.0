import { randomUUID } from "node:crypto";
import type { Database } from "./client.js";

/** A partner-access application: an ordinary account asking to join the partner program. */
export interface ReferralApplication {
  id: string;
  userId: string;
  email: string;
  status: "pending" | "approved" | "rejected";
  message: string;
  reviewerActor: string | null;
  reviewerNote: string | null;
  decidedAt: string | null;
  createdAt: string;
}

interface ReferralApplicationRow {
  id: string;
  user_id: string;
  email: string;
  status: "pending" | "approved" | "rejected";
  message: string;
  reviewer_actor: string | null;
  reviewer_note: string | null;
  decided_at: Date | null;
  created_at: Date;
}

const PROJECTION = `
  SELECT a.id, a.user_id, u.email, a.status, a.message,
         a.reviewer_actor, a.reviewer_note, a.decided_at, a.created_at
  FROM referral_applications a
  JOIN users u ON u.id = a.user_id
`;

function mapApplication(row: ReferralApplicationRow): ReferralApplication {
  return {
    id: row.id,
    userId: row.user_id,
    email: row.email,
    status: row.status,
    message: row.message,
    reviewerActor: row.reviewer_actor,
    reviewerNote: row.reviewer_note,
    decidedAt: row.decided_at ? row.decided_at.toISOString() : null,
    createdAt: row.created_at.toISOString(),
  };
}

/**
 * Submit an application. One open application per account: submitting again refreshes the
 * message of the pending row instead of queueing a second review.
 */
export async function submitReferralApplication(
  database: Database,
  input: { userId: string; message: string },
): Promise<ReferralApplication> {
  const message = input.message.trim().slice(0, 2_000);
  const result = await database.pool.query<{ id: string }>(`
    INSERT INTO referral_applications (id, user_id, message)
    VALUES ($1, $2, $3)
    ON CONFLICT (user_id) WHERE status = 'pending'
    DO UPDATE SET message = EXCLUDED.message, updated_at = now()
    RETURNING id
  `, [randomUUID(), input.userId, message]);
  const id = result.rows[0]!.id;
  const stored = await database.pool.query<ReferralApplicationRow>(`${PROJECTION} WHERE a.id = $1`, [id]);
  return mapApplication(stored.rows[0]!);
}

/** The account's own latest application, whatever its state. */
export async function findLatestReferralApplication(
  database: Database,
  userId: string,
): Promise<ReferralApplication | null> {
  const result = await database.pool.query<ReferralApplicationRow>(`${PROJECTION}
    WHERE a.user_id = $1
    ORDER BY a.created_at DESC
    LIMIT 1
  `, [userId]);
  return result.rows[0] ? mapApplication(result.rows[0]) : null;
}

/** The admin review queue, newest first. */
export async function listReferralApplications(
  database: Database,
  query: { status?: "pending" | "approved" | "rejected" | undefined; limit?: number | undefined } = {},
): Promise<ReferralApplication[]> {
  const limit = Math.min(Math.max(query.limit ?? 100, 1), 200);
  const result = query.status
    ? await database.pool.query<ReferralApplicationRow>(`${PROJECTION}
        WHERE a.status = $1 ORDER BY a.created_at DESC LIMIT $2
      `, [query.status, limit])
    : await database.pool.query<ReferralApplicationRow>(`${PROJECTION}
        ORDER BY (a.status = 'pending') DESC, a.created_at DESC LIMIT $1
      `, [limit]);
  return result.rows.map(mapApplication);
}

export async function findReferralApplication(
  database: Database,
  id: string,
): Promise<ReferralApplication | null> {
  const result = await database.pool.query<ReferralApplicationRow>(`${PROJECTION} WHERE a.id = $1`, [id]);
  return result.rows[0] ? mapApplication(result.rows[0]) : null;
}

/**
 * Record a decision. Only a pending application can be decided, so a repeated approval cannot
 * onboard the same account twice through this path.
 */
export async function decideReferralApplication(
  database: Database,
  input: { id: string; status: "approved" | "rejected"; reviewerActor: string; reviewerNote: string },
): Promise<ReferralApplication | null> {
  const result = await database.pool.query<{ id: string }>(`
    UPDATE referral_applications
    SET status = $2, reviewer_actor = $3, reviewer_note = $4, decided_at = now(), updated_at = now()
    WHERE id = $1 AND status = 'pending'
    RETURNING id
  `, [input.id, input.status, input.reviewerActor, input.reviewerNote.trim().slice(0, 2_000)]);
  if (!result.rows[0]) return null;
  return findReferralApplication(database, input.id);
}
