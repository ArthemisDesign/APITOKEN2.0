import { randomUUID } from "node:crypto";
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
  referralCode: string | null;
}

export async function createOAuthTransaction(database: Database, input: {
  stateHash: string;
  provider: OAuthProvider;
  nonce: string | null;
  codeVerifier: string;
  inviteTokenHash: string | null;
  referralCode: string | null;
  expiresAt: Date;
}): Promise<void> {
  await database.pool.query(`
    DELETE FROM oauth_transactions
    WHERE expires_at < now() - interval '1 day' OR consumed_at < now() - interval '1 day'
  `);
  await database.pool.query(`
    INSERT INTO oauth_transactions (
      state_hash, provider, nonce, code_verifier, invite_token_hash, referral_code, expires_at
    ) VALUES ($1, $2, $3, $4, $5, $6, $7)
  `, [input.stateHash, input.provider, input.nonce, input.codeVerifier, input.inviteTokenHash, input.referralCode, input.expiresAt]);
}

export async function consumeOAuthTransaction(
  database: Database,
  stateHash: string,
  provider: OAuthProvider,
): Promise<OAuthTransaction | null> {
  const result = await database.pool.query<{
    provider: OAuthProvider; nonce: string | null; code_verifier: string;
    invite_token_hash: string | null; referral_code: string | null;
  }>(`
    UPDATE oauth_transactions SET consumed_at = now()
    WHERE state_hash = $1 AND provider = $2 AND consumed_at IS NULL AND expires_at > now()
    RETURNING provider, nonce, code_verifier, invite_token_hash, referral_code
  `, [stateHash, provider]);
  const row = result.rows[0];
  return row ? {
    provider: row.provider,
    nonce: row.nonce,
    codeVerifier: row.code_verifier,
    inviteTokenHash: row.invite_token_hash,
    referralCode: row.referral_code,
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

/**
 * true, если у юзера есть OAuth-идентичность (Google/GitHub) — маркер OAuth-регистрации.
 * Lookup по индексу auth_identities_user_provider_uidx; у password-аккаунтов записей нет.
 */
export async function hasOAuthIdentity(database: Database, userId: string): Promise<boolean> {
  const result = await database.pool.query(
    "SELECT 1 FROM auth_identities WHERE user_id = $1 LIMIT 1",
    [userId],
  );
  return (result.rowCount ?? 0) > 0;
}

export async function completeExternalSignIn(
  database: Database,
  identity: VerifiedExternalIdentity,
  businessInviteTokenHash: string | null,
): Promise<RegisteredAuthUser> {
  const existing = await findExternalAuthUser(database, identity.provider, identity.subject);
  if (existing) return { ...existing, engineMultiplierBp: await multiplierForUser(database, existing.id), isNewAccount: false };

  const client = await database.pool.connect();
  const userId = randomUUID();
  try {
    await client.query("BEGIN");
    const emailOwner = await client.query<{
      id: string;
      email: string;
      display_name: string;
      status: "active" | "disabled";
      engine_account_status: "pending" | "active" | "error" | "disabled";
      customer_type: "b2c" | "b2b";
      totp_enabled: boolean;
      engine_multiplier_bp: number;
    }>(`
      SELECT u.id, u.email, u.display_name, u.status, u.totp_enabled,
             COALESCE(ea.status, 'pending') AS engine_account_status,
             COALESCE(cp.customer_type, 'b2c') AS customer_type,
             COALESCE(ea.mult_bp, $2) AS engine_multiplier_bp
      FROM users u
      LEFT JOIN engine_accounts ea ON ea.user_id = u.id
      LEFT JOIN customer_profiles cp ON cp.user_id = u.id
      WHERE lower(u.email) = lower($1)
      FOR UPDATE OF u
    `, [identity.email, 5_000]);
    const claimedAccount = emailOwner.rows[0];
    if (claimedAccount) {
      if (claimedAccount.status !== "active") throw new Error("account is disabled");
      await client.query(`
        INSERT INTO auth_identities (id, user_id, provider, subject, email, email_verified, metadata)
        VALUES ($1, $2, $3, $4, $5, true, $6::jsonb)
      `, [randomUUID(), claimedAccount.id, identity.provider, identity.subject, identity.email, JSON.stringify({
        displayName: identity.displayName,
        ...identity.metadata,
      })]);
      await client.query(`
        UPDATE users
        SET password_hash = NULL, email_verified = true, updated_at = now()
        WHERE id = $1
      `, [claimedAccount.id]);
      await client.query(`
        UPDATE auth_sessions
        SET revoked_at = now()
        WHERE user_id = $1 AND revoked_at IS NULL
      `, [claimedAccount.id]);
      await client.query(`
        UPDATE auth_tokens
        SET used_at = now()
        WHERE user_id = $1 AND used_at IS NULL
      `, [claimedAccount.id]);
      await client.query(`
        INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
        VALUES ('provider', $1, 'auth.oauth_claimed', 'user', $2, $3::jsonb)
      `, [identity.provider, claimedAccount.id, JSON.stringify({ provider: identity.provider })]);
      await client.query("COMMIT");
      return {
        id: claimedAccount.id,
        email: claimedAccount.email,
        displayName: claimedAccount.display_name,
        emailVerified: true,
        passwordEnabled: false,
        status: claimedAccount.status,
        engineAccountStatus: claimedAccount.engine_account_status,
        customerType: claimedAccount.customer_type,
        totpEnabled: claimedAccount.totp_enabled,
        engineMultiplierBp: claimedAccount.engine_multiplier_bp,
        isNewAccount: false,
      };
    }
    const invite = businessInviteTokenHash
      ? await lockBusinessInvite(client, { email: identity.email, tokenHash: businessInviteTokenHash })
      : null;
    const customerType = invite ? "b2b" : "b2c";
    // New B2C registrations get the standard 50% off; a B2B invitee gets exactly the discount
    // their invitation carries. The invitation multiplier IS the negotiated price now — there is
    // no policy document behind it, so inheriting full price here would silently overcharge.
    const engineMultiplierBp = invite ? invite.multiplierBp : 5_000;
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
        UPDATE business_invites
        SET consumed_at = now(), consumed_by_user_id = $2, encrypted_token = NULL
        WHERE id = $1 AND consumed_at IS NULL
      `, [invite.id, userId]);
      await client.query(`
        UPDATE email_outbox
        SET status = 'canceled', locked_at = NULL, locked_by = NULL,
            last_error = 'business invitation consumed', updated_at = now()
        WHERE business_invite_id = $1 AND status IN ('pending', 'processing')
      `, [invite.id]);
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
      isNewAccount: true,
    };
  } catch (error) {
    await client.query("ROLLBACK");
    if (isUniqueViolation(error)) {
      const raced = await findExternalAuthUser(database, identity.provider, identity.subject);
      if (raced) return { ...raced, engineMultiplierBp: await multiplierForUser(database, raced.id), isNewAccount: false };
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
  return result.rows[0]?.mult_bp ?? 5_000;
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
