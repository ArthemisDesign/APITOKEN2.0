import { randomUUID } from "node:crypto";
import type { PoolClient } from "pg";
import type { Database } from "./client.js";

export const MANAGED_ADMIN_DOMAINS = [
  "admin.apitoken.sale",
  "admin.partners.apitoken.sale",
  "crm.apitoken.sale",
  "content-studio.apitoken.sale",
] as const;

export type ManagedAdminDomain = typeof MANAGED_ADMIN_DOMAINS[number];
export type ManagedAdminStatus = "active" | "disabled";

export interface ManagedAdminAccount {
  id: string;
  username: string;
  status: ManagedAdminStatus;
  domains: ManagedAdminDomain[];
  passwordChangedAt: Date | null;
  createdAt: Date;
  updatedAt: Date;
}

export interface ManagedAdminCredential extends ManagedAdminAccount {
  passwordHash: string;
}

export class ManagedAdminConflictError extends Error {}
export class ManagedAdminNotFoundError extends Error {}
export class LastMainAdminError extends Error {}
export class LegacyAdminImportConflictError extends Error {}

export async function listManagedAdminAccounts(
  database: Database,
  domain?: ManagedAdminDomain,
): Promise<ManagedAdminAccount[]> {
  const result = await database.pool.query<AdminAccountRow>(`
    SELECT account.id, account.username, account.status, account.password_changed_at,
           account.created_at, account.updated_at,
           array_agg(all_grants.domain ORDER BY all_grants.domain) AS domains
    FROM admin_accounts account
    JOIN admin_account_domains all_grants ON all_grants.admin_account_id = account.id
    WHERE ($1::text IS NULL OR EXISTS (
      SELECT 1 FROM admin_account_domains requested_grant
      WHERE requested_grant.admin_account_id = account.id AND requested_grant.domain = $1
    ))
    GROUP BY account.id
    ORDER BY lower(account.username), account.id
  `, [domain ?? null]);
  return result.rows.map(mapAdminAccount);
}

export async function findManagedAdminCredential(
  database: Database,
  input: { username: string; domain: ManagedAdminDomain },
): Promise<ManagedAdminCredential | null> {
  const result = await database.pool.query<AdminCredentialRow>(`
    SELECT account.id, account.username, account.password_hash, account.status,
           account.password_changed_at, account.created_at, account.updated_at,
           array_agg(all_grants.domain ORDER BY all_grants.domain) AS domains
    FROM admin_accounts account
    JOIN admin_account_domains requested_grant
      ON requested_grant.admin_account_id = account.id AND requested_grant.domain = $2
    JOIN admin_account_domains all_grants ON all_grants.admin_account_id = account.id
    WHERE lower(account.username) = lower($1)
    GROUP BY account.id
  `, [input.username, input.domain]);
  const row = result.rows[0];
  return row ? { ...mapAdminAccount(row), passwordHash: row.password_hash } : null;
}

export async function createManagedAdminAccount(
  database: Database,
  input: {
    username: string;
    passwordHash: string;
    domains: readonly ManagedAdminDomain[];
    actorId: string;
    reason: string;
  },
): Promise<ManagedAdminAccount> {
  return withAdminTransaction(database, async (client) => {
    await lockAdminMutations(client);
    const id = randomUUID();
    try {
      await client.query(`
        INSERT INTO admin_accounts (id, username, password_hash, password_changed_at)
        VALUES ($1, $2, $3, now())
      `, [id, input.username, input.passwordHash]);
    } catch (error) {
      if (isUniqueViolation(error)) throw new ManagedAdminConflictError("admin username already exists");
      throw error;
    }
    await replaceDomains(client, id, input.domains);
    await recordAdminAudit(client, {
      actorId: input.actorId,
      action: "admin_account.created",
      targetId: id,
      metadata: { username: input.username, domains: input.domains, reason: input.reason },
    });
    return requireAdminAccount(client, id);
  });
}

export async function changeManagedAdminPassword(
  database: Database,
  input: { accountId: string; passwordHash: string; actorId: string; reason: string },
): Promise<ManagedAdminAccount> {
  return withAdminTransaction(database, async (client) => {
    const updated = await client.query<{ username: string }>(`
      UPDATE admin_accounts
      SET password_hash = $2, password_changed_at = now(), updated_at = now()
      WHERE id = $1
      RETURNING username
    `, [input.accountId, input.passwordHash]);
    const row = updated.rows[0];
    if (!row) throw new ManagedAdminNotFoundError("admin account not found");
    await recordAdminAudit(client, {
      actorId: input.actorId,
      action: "admin_account.password_changed",
      targetId: input.accountId,
      metadata: { username: row.username, reason: input.reason },
    });
    return requireAdminAccount(client, input.accountId);
  });
}

export async function setManagedAdminDomains(
  database: Database,
  input: {
    accountId: string;
    domains: readonly ManagedAdminDomain[];
    actorId: string;
    reason: string;
  },
): Promise<ManagedAdminAccount> {
  return withAdminTransaction(database, async (client) => {
    await lockAdminMutations(client);
    const before = await requireAdminAccount(client, input.accountId);
    if (before.status === "active" && before.domains.includes("admin.apitoken.sale") &&
        !input.domains.includes("admin.apitoken.sale")) {
      await requireAnotherMainAdmin(client, input.accountId);
    }
    await replaceDomains(client, input.accountId, input.domains);
    await client.query("UPDATE admin_accounts SET updated_at = now() WHERE id = $1", [input.accountId]);
    await recordAdminAudit(client, {
      actorId: input.actorId,
      action: "admin_account.domains_changed",
      targetId: input.accountId,
      metadata: {
        username: before.username,
        before: before.domains,
        after: input.domains,
        reason: input.reason,
      },
    });
    return requireAdminAccount(client, input.accountId);
  });
}

export async function setManagedAdminStatus(
  database: Database,
  input: { accountId: string; status: ManagedAdminStatus; actorId: string; reason: string },
): Promise<ManagedAdminAccount> {
  return withAdminTransaction(database, async (client) => {
    await lockAdminMutations(client);
    const before = await requireAdminAccount(client, input.accountId);
    if (before.status === "active" && input.status === "disabled" &&
        before.domains.includes("admin.apitoken.sale")) {
      await requireAnotherMainAdmin(client, input.accountId);
    }
    await client.query(`
      UPDATE admin_accounts SET status = $2, updated_at = now() WHERE id = $1
    `, [input.accountId, input.status]);
    await recordAdminAudit(client, {
      actorId: input.actorId,
      action: "admin_account.status_changed",
      targetId: input.accountId,
      metadata: {
        username: before.username,
        before: before.status,
        after: input.status,
        reason: input.reason,
      },
    });
    return requireAdminAccount(client, input.accountId);
  });
}

export async function importLegacyAdminAccounts(
  database: Database,
  input: readonly {
    username: string;
    passwordHash: string;
    domains: readonly ManagedAdminDomain[];
  }[],
): Promise<{ imported: number; accounts: number; mainAdminAccounts: number; crmAccounts: number }> {
  return withAdminTransaction(database, async (client) => {
    await lockAdminMutations(client);
    let imported = 0;
    for (const legacy of input) {
      const existing = await client.query<{
        id: string;
        password_hash: string;
        password_changed_at: Date | null;
      }>(`
        SELECT id, password_hash, password_changed_at
        FROM admin_accounts WHERE lower(username) = lower($1) FOR UPDATE
      `, [legacy.username]);
      let accountId: string;
      if (existing.rows[0]) {
        const row = existing.rows[0];
        if (row.password_changed_at === null && row.password_hash !== legacy.passwordHash) {
          throw new LegacyAdminImportConflictError(
            `legacy username ${legacy.username} has conflicting password hashes`,
          );
        }
        accountId = row.id;
      } else {
        accountId = randomUUID();
        await client.query(`
          INSERT INTO admin_accounts (id, username, password_hash)
          VALUES ($1, $2, $3)
        `, [accountId, legacy.username, legacy.passwordHash]);
        imported += 1;
      }
      for (const domain of legacy.domains) {
        await client.query(`
          INSERT INTO admin_account_domains (admin_account_id, domain)
          VALUES ($1, $2) ON CONFLICT DO NOTHING
        `, [accountId, domain]);
      }
    }
    const counts = await client.query<{
      count: string;
      main_admin_accounts: string;
      crm_accounts: string;
    }>(`
      SELECT count(DISTINCT account.id)::text AS count,
             count(DISTINCT account.id) FILTER (
               WHERE account.status = 'active' AND access_grant.domain = 'admin.apitoken.sale'
             )::text AS main_admin_accounts,
             count(DISTINCT account.id) FILTER (
               WHERE account.status = 'active' AND access_grant.domain = 'crm.apitoken.sale'
             )::text AS crm_accounts
      FROM admin_accounts account
      LEFT JOIN admin_account_domains access_grant ON access_grant.admin_account_id = account.id
    `);
    const count = counts.rows[0];
    const mainAdminAccounts = Number(count?.main_admin_accounts ?? 0);
    const crmAccounts = Number(count?.crm_accounts ?? 0);
    if (mainAdminAccounts === 0 || crmAccounts === 0) {
      throw new LegacyAdminImportConflictError(
        "legacy import must preserve at least one active main-admin and CRM account",
      );
    }
    await client.query(`
      INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('system', 'caddy-migration', 'admin_account.legacy_imported',
              'admin_account', 'legacy-caddy', $1::jsonb)
    `, [JSON.stringify({ rows: input.length, imported })]);
    return {
      imported,
      accounts: Number(count?.count ?? 0),
      mainAdminAccounts,
      crmAccounts,
    };
  });
}

export async function upgradeLegacyAdminPasswordHash(
  database: Database,
  input: { accountId: string; previousHash: string; passwordHash: string },
): Promise<void> {
  await database.pool.query(`
    UPDATE admin_accounts
    SET password_hash = $3, password_changed_at = now(), updated_at = now()
    WHERE id = $1 AND password_hash = $2 AND password_changed_at IS NULL
  `, [input.accountId, input.previousHash, input.passwordHash]);
}

async function withAdminTransaction<T>(
  database: Database,
  action: (client: PoolClient) => Promise<T>,
): Promise<T> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const result = await action(client);
    await client.query("COMMIT");
    return result;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

async function lockAdminMutations(client: PoolClient): Promise<void> {
  await client.query("SELECT pg_advisory_xact_lock(hashtext('managed-admin-accounts'))");
}

async function replaceDomains(
  client: PoolClient,
  accountId: string,
  domains: readonly ManagedAdminDomain[],
): Promise<void> {
  await client.query("DELETE FROM admin_account_domains WHERE admin_account_id = $1", [accountId]);
  for (const domain of domains) {
    await client.query(`
      INSERT INTO admin_account_domains (admin_account_id, domain) VALUES ($1, $2)
    `, [accountId, domain]);
  }
}

async function requireAnotherMainAdmin(client: PoolClient, excludedId: string): Promise<void> {
  const result = await client.query<{ exists: boolean }>(`
    SELECT EXISTS (
      SELECT 1 FROM admin_accounts account
      JOIN admin_account_domains access_grant ON access_grant.admin_account_id = account.id
      WHERE account.status = 'active' AND access_grant.domain = 'admin.apitoken.sale' AND account.id <> $1
    ) AS exists
  `, [excludedId]);
  if (!result.rows[0]?.exists) {
    throw new LastMainAdminError("cannot remove or disable the last active main-admin account");
  }
}

async function requireAdminAccount(client: PoolClient, accountId: string): Promise<ManagedAdminAccount> {
  const result = await client.query<AdminAccountRow>(`
    SELECT account.id, account.username, account.status, account.password_changed_at,
           account.created_at, account.updated_at,
           array_agg(access_grant.domain ORDER BY access_grant.domain) AS domains
    FROM admin_accounts account
    JOIN admin_account_domains access_grant ON access_grant.admin_account_id = account.id
    WHERE account.id = $1
    GROUP BY account.id
  `, [accountId]);
  const row = result.rows[0];
  if (!row) throw new ManagedAdminNotFoundError("admin account not found");
  return mapAdminAccount(row);
}

async function recordAdminAudit(
  client: PoolClient,
  input: { actorId: string; action: string; targetId: string; metadata: Record<string, unknown> },
): Promise<void> {
  await client.query(`
    INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
    VALUES ('admin', $1, $2, 'admin_account', $3, $4::jsonb)
  `, [input.actorId, input.action, input.targetId, JSON.stringify(input.metadata)]);
}

function mapAdminAccount(row: AdminAccountRow): ManagedAdminAccount {
  return {
    id: row.id,
    username: row.username,
    status: row.status,
    domains: row.domains,
    passwordChangedAt: row.password_changed_at,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

interface AdminAccountRow {
  id: string;
  username: string;
  status: ManagedAdminStatus;
  domains: ManagedAdminDomain[];
  password_changed_at: Date | null;
  created_at: Date;
  updated_at: Date;
}

interface AdminCredentialRow extends AdminAccountRow {
  password_hash: string;
}

function isUniqueViolation(error: unknown): boolean {
  return Boolean(error && typeof error === "object" && "code" in error && error.code === "23505");
}
