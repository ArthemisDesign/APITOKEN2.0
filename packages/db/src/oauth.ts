import { randomUUID } from "node:crypto";
import { B2C_PRICING_TIERS } from "@claude-api/contracts";
import type { Database } from "./client.js";
import { initialDisplayName, type AuthUser, type RegisteredAuthUser } from "./auth.js";
import { lockBusinessInvite, utcMonthStart } from "./pricing.js";

export type OAuthProvider = "google" | "github";

export interface VerifiedExternalIdentity {
  provider: OAuthProvider;
  subject: string;
  email: string;
  emailVerified: true;
  displayName: string | null;
  metadata: Readonly<Record<string, unknown>>;
}

export interface OAuthTransaction {
  provider: OAuthProvider;
  nonce: string | null;
  codeVerifier: string;
  inviteTokenHash: string | null;
}

export class ExternalIdentityLinkRequiredError extends Error {}

export async function createOAuthTransaction(database: Database, input: {
  stateHash: string;
  provider: OAuthProvider;
  nonce: string | null;
  codeVerifier: string;
  inviteTokenHash: string | null;
  expiresAt: Date;
}): Promise<void> {
  await database.pool.query(`
    DELETE FROM oauth_transactions
    WHERE expires_at < now() - interval '1 day' OR consumed_at < now() - interval '1 day'
  `);
  await database.pool.query(`
    INSERT INTO oauth_transactions (
      state_hash, provider, nonce, code_verifier, invite_token_hash, expires_at
    ) VALUES ($1, $2, $3, $4, $5, $6)
  `, [input.stateHash, input.provider, input.nonce, input.codeVerifier, input.inviteTokenHash, input.expiresAt]);
}

export async function consumeOAuthTransaction(
  database: Database,
  stateHash: string,
  provider: OAuthProvider,
): Promise<OAuthTransaction | null> {
  const result = await database.pool.query<{
    provider: OAuthProvider; nonce: string | null; code_verifier: string; invite_token_hash: string | null;
  }>(`
    UPDATE oauth_transactions SET consumed_at = now()
    WHERE state_hash = $1 AND provider = $2 AND consumed_at IS NULL AND expires_at > now()
    RETURNING provider, nonce, code_verifier, invite_token_hash
  `, [stateHash, provider]);
  const row = result.rows[0];
  return row ? {
    provider: row.provider,
    nonce: row.nonce,
    codeVerifier: row.code_verifier,
    inviteTokenHash: row.invite_token_hash,
  } : null;
}

export async function findExternalAuthUser(
  database: Database,
  provider: OAuthProvider,
  subject: string,
): Promise<AuthUser | null> {
  const result = await database.pool.query<ExternalUserRow>(`
    SELECT u.id, u.email, u.display_name, u.email_verified, u.status, u.totp_enabled,
           (u.password_hash IS NOT NULL) AS password_enabled,
           COALESCE(ea.status, 'pending') AS engine_account_status,
           COALESCE(cp.customer_type, 'b2c') AS customer_type
    FROM auth_identities ai JOIN users u ON u.id = ai.user_id
    LEFT JOIN engine_accounts ea ON ea.user_id = u.id
    LEFT JOIN customer_profiles cp ON cp.user_id = u.id
    WHERE ai.provider = $1 AND ai.subject = $2
  `, [provider, subject]);
  return result.rows[0] ? mapExternalUser(result.rows[0]) : null;
}

export async function completeExternalSignIn(
  database: Database,
  identity: VerifiedExternalIdentity,
  businessInviteTokenHash: string | null,
): Promise<RegisteredAuthUser> {
  const existing = await findExternalAuthUser(database, identity.provider, identity.subject);
  if (existing) return { ...existing, engineMultiplierBp: await multiplierForUser(database, existing.id) };

  const client = await database.pool.connect();
  const userId = randomUUID();
  try {
    await client.query("BEGIN");
    const emailOwner = await client.query<{ id: string }>(`
      SELECT id FROM users WHERE lower(email) = lower($1) FOR UPDATE
    `, [identity.email]);
    if (emailOwner.rows[0]) {
      throw new ExternalIdentityLinkRequiredError(
        "an account with this email already exists; sign in to that account before linking this provider",
      );
    }
    const invite = businessInviteTokenHash
      ? await lockBusinessInvite(client, { email: identity.email, tokenHash: businessInviteTokenHash })
      : null;
    const customerType = invite ? "b2b" : "b2c";
    const engineMultiplierBp = invite?.multiplierBp ?? B2C_PRICING_TIERS[0].multiplierBp;
    const monthStart = utcMonthStart();
    await client.query(`
      INSERT INTO users (id, email, display_name, email_verified, password_hash) VALUES ($1, $2, $3, true, NULL)
    `, [userId, identity.email, initialDisplayName(identity.email, identity.displayName)]);
    await client.query(`
      INSERT INTO auth_identities (id, user_id, provider, subject, email, email_verified, metadata)
      VALUES ($1, $2, $3, $4, $5, true, $6::jsonb)
    `, [randomUUID(), userId, identity.provider, identity.subject, identity.email, JSON.stringify({
      displayName: identity.displayName,
      ...identity.metadata,
    })]);
    await client.query(`
      INSERT INTO engine_accounts (id, user_id, mult_bp, status) VALUES ($1, $2, $3, 'pending')
    `, [randomUUID(), userId, engineMultiplierBp]);
    await client.query(`
      INSERT INTO customer_profiles (user_id, customer_type, current_tier, multiplier_bp, pricing_month_start)
      VALUES ($1, $2, $3, $4, $5)
    `, [userId, customerType, invite ? null : 0, engineMultiplierBp, monthStart]);
    if (invite) {
      await client.query(`
        UPDATE business_invites SET consumed_at = now(), consumed_by_user_id = $2
        WHERE id = $1 AND consumed_at IS NULL
      `, [invite.id, userId]);
    } else {
      await client.query(`
        INSERT INTO pricing_months (id, user_id, month_start, opening_tier, highest_tier)
        VALUES ($1, $2, $3, 0, 0)
      `, [randomUUID(), userId, monthStart]);
    }
    await client.query(`
      INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('provider', $1, 'auth.oauth_registered', 'user', $2, $3::jsonb)
    `, [identity.provider, userId, JSON.stringify({ provider: identity.provider })]);
    await client.query("COMMIT");
    return {
      id: userId,
      email: identity.email,
      displayName: initialDisplayName(identity.email, identity.displayName),
      emailVerified: true,
      passwordEnabled: false,
      status: "active",
      engineAccountStatus: "pending",
      customerType,
      totpEnabled: false,
      engineMultiplierBp,
    };
  } catch (error) {
    await client.query("ROLLBACK");
    if (isUniqueViolation(error)) {
      const raced = await findExternalAuthUser(database, identity.provider, identity.subject);
      if (raced) return { ...raced, engineMultiplierBp: await multiplierForUser(database, raced.id) };
    }
    throw error;
  } finally {
    client.release();
  }
}

async function multiplierForUser(database: Database, userId: string): Promise<number> {
  const result = await database.pool.query<{ mult_bp: number }>(
    "SELECT mult_bp FROM engine_accounts WHERE user_id = $1",
    [userId],
  );
  return result.rows[0]?.mult_bp ?? B2C_PRICING_TIERS[0].multiplierBp;
}

interface ExternalUserRow {
  id: string;
  email: string;
  display_name: string;
  email_verified: boolean;
  password_enabled: boolean;
  status: "active" | "disabled";
  engine_account_status: "pending" | "active" | "error" | "disabled";
  customer_type: "b2c" | "b2b";
  totp_enabled: boolean;
}

function mapExternalUser(row: ExternalUserRow): AuthUser {
  return {
    id: row.id,
    email: row.email,
    displayName: row.display_name,
    emailVerified: row.email_verified,
    passwordEnabled: row.password_enabled,
    status: row.status,
    engineAccountStatus: row.engine_account_status,
    customerType: row.customer_type,
    totpEnabled: row.totp_enabled,
  };
}

function isUniqueViolation(error: unknown): boolean {
  return typeof error === "object" && error !== null && "code" in error && error.code === "23505";
}
