import { randomBytes } from "node:crypto";
import type { PoolClient } from "pg";
import type { SalesDatabase } from "./client.js";

export interface ExternalReferralAlias {
  source: string;
  externalRef: string;
  aliasCode: string;
  partnerId: string;
  createdAt: Date;
}

export class ExternalReferralAliasOwnerNotFoundError extends Error {}
export class ExternalReferralAliasConflictError extends Error {}

interface AliasRow {
  source: string;
  external_ref: string;
  alias_code: string;
  partner_id: string;
  created_at: Date;
}

function mapAlias(row: AliasRow): ExternalReferralAlias {
  return {
    source: row.source,
    externalRef: row.external_ref,
    aliasCode: row.alias_code,
    partnerId: row.partner_id,
    createdAt: row.created_at,
  };
}

function publicAliasCode(): string {
  return `r_${randomBytes(18).toString("base64url").toLowerCase()}`;
}

/**
 * Idempotently binds one source-owned opaque reference to an active partner. The binding never
 * moves to another partner: a caller replay gets the original alias and a changed owner fails.
 */
export async function ensureExternalReferralAlias(database: SalesDatabase, input: {
  source: string;
  externalRef: string;
  partnerReferralCode: string;
}): Promise<ExternalReferralAlias> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const owner = await client.query<{ id: string }>(
      "SELECT id FROM partners WHERE referral_code = $1 AND status = 'active' FOR SHARE",
      [input.partnerReferralCode],
    );
    const partnerId = owner.rows[0]?.id;
    if (!partnerId) {
      await client.query("ROLLBACK");
      throw new ExternalReferralAliasOwnerNotFoundError("active referral owner not found");
    }

    const existing = await selectAlias(client, input.source, input.externalRef);
    if (existing) {
      if (existing.partner_id !== partnerId) {
        await client.query("ROLLBACK");
        throw new ExternalReferralAliasConflictError("external referral reference already belongs to another partner");
      }
      await client.query("COMMIT");
      return mapAlias(existing);
    }

    for (let attempt = 0; attempt < 4; attempt += 1) {
      await client.query("SAVEPOINT external_referral_alias_code_attempt");
      try {
        const aliasCode = publicAliasCode();
        const inserted = await client.query<AliasRow>(`
          INSERT INTO external_referral_aliases (source, external_ref, alias_code, partner_id)
          SELECT $1, $2, $3, $4
          WHERE NOT EXISTS (SELECT 1 FROM partners WHERE referral_code = $3)
            AND NOT EXISTS (SELECT 1 FROM partner_discount_links WHERE code = $3)
          ON CONFLICT (source, external_ref) DO NOTHING
          RETURNING source, external_ref, alias_code, partner_id, created_at
        `, [input.source, input.externalRef, aliasCode, partnerId]);
        await client.query("RELEASE SAVEPOINT external_referral_alias_code_attempt");
        const row = inserted.rows[0] ?? await selectAlias(client, input.source, input.externalRef);
        if (!row) continue;
        if (row.partner_id !== partnerId) {
          await client.query("ROLLBACK");
          throw new ExternalReferralAliasConflictError("external referral reference already belongs to another partner");
        }
        await client.query("COMMIT");
        return mapAlias(row);
      } catch (error) {
        if (!isUniqueViolation(error)) throw error;
        await client.query("ROLLBACK TO SAVEPOINT external_referral_alias_code_attempt");
      }
    }
    throw new Error("could not allocate a unique external referral alias");
  } catch (error) {
    await client.query("ROLLBACK").catch(() => {});
    throw error;
  } finally {
    client.release();
  }
}

export async function resolveExternalReferralAlias(
  database: SalesDatabase,
  aliasCode: string,
): Promise<{ partnerId: string } | null> {
  const result = await database.pool.query<{ partner_id: string }>(`
    SELECT alias.partner_id
    FROM external_referral_aliases alias
    JOIN partners partner ON partner.id = alias.partner_id
    WHERE alias.alias_code = $1 AND partner.status = 'active'
  `, [aliasCode]);
  return result.rows[0] ? { partnerId: result.rows[0].partner_id } : null;
}

async function selectAlias(client: PoolClient, source: string, externalRef: string): Promise<AliasRow | null> {
  const result = await client.query<AliasRow>(`
    SELECT source, external_ref, alias_code, partner_id, created_at
    FROM external_referral_aliases
    WHERE source = $1 AND external_ref = $2
  `, [source, externalRef]);
  return result.rows[0] ?? null;
}

function isUniqueViolation(error: unknown): boolean {
  return typeof error === "object" && error !== null && "code" in error
    && (error as { code?: string }).code === "23505";
}
