import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { hash as bcryptHash } from "bcryptjs";
import {
  createDatabase,
  LastMainAdminError,
  type Database,
  type ManagedAdminDomain,
} from "@claude-api/db";
import { AdminAccountsService } from "./admin-accounts.service.js";

const connectionString = process.env.TEST_DATABASE_URL;

describe.runIf(Boolean(connectionString))("managed admin accounts", () => {
  let database: Database;
  let service: AdminAccountsService;

  beforeAll(() => {
    database = createDatabase(connectionString!);
    service = new AdminAccountsService(database);
  });

  beforeEach(async () => {
    await database.pool.query(`
      TRUNCATE admin_account_domains, admin_accounts, audit_log RESTART IDENTITY CASCADE
    `);
  });

  afterAll(async () => {
    await database.pool.end();
  });

  it("creates multi-domain accounts, filters exactly, and authenticates only granted domains", async () => {
    const created = await service.create({
      username: "Q.Admin",
      password: "correct horse battery staple",
      domains: ["admin.apitoken.sale", "admin.partners.apitoken.sale"],
      actorId: "legacy-q",
      reason: "add platform operator",
    }) as unknown as AccountResponse;
    await service.create({
      username: "CRM.Admin",
      password: "another correct battery staple",
      domains: ["crm.apitoken.sale"],
      actorId: created.account.id,
      reason: "add CRM operator",
    });

    const filtered = await service.list("admin.partners.apitoken.sale") as unknown as AccountListResponse;
    expect(filtered.accounts).toHaveLength(1);
    expect(filtered.accounts[0]).toMatchObject({
      username: "Q.Admin",
      domains: ["admin.apitoken.sale", "admin.partners.apitoken.sale"],
    });
    await expect(service.authenticate({
      authorization: basic("q.admin", "correct horse battery staple"),
      domain: "admin.apitoken.sale",
    })).resolves.toMatchObject({ id: created.account.id, username: "Q.Admin" });
    await expect(service.authenticate({
      authorization: basic("Q.Admin", "correct horse battery staple"),
      domain: "crm.apitoken.sale",
    })).resolves.toBeNull();
  });

  it("rotates any password including self and never returns a password hash", async () => {
    const created = await createMain(service);
    const result = await service.changePassword({
      accountId: created.account.id,
      password: "the replacement battery staple",
      actorId: created.account.id,
      reason: "scheduled credential rotation",
    }) as unknown as AccountResponse & { changed_self: boolean };
    expect(result.changed_self).toBe(true);
    expect(JSON.stringify(result)).not.toContain("password_hash");
    await expect(service.authenticate({
      authorization: basic("main-admin", "correct horse battery staple"),
      domain: "admin.apitoken.sale",
    })).resolves.toBeNull();
    await expect(service.authenticate({
      authorization: basic("main-admin", "the replacement battery staple"),
      domain: "admin.apitoken.sale",
    })).resolves.toMatchObject({ id: created.account.id });
    const audit = await database.pool.query(`
      SELECT actor_id, metadata FROM audit_log WHERE action = 'admin_account.password_changed'
    `);
    expect(audit.rows).toEqual([{
      actor_id: created.account.id,
      metadata: expect.objectContaining({ reason: "scheduled credential rotation" }),
    }]);
  });

  it("revokes persistent sessions after a password or domain-grant change", async () => {
    const created = await service.create({
      username: "session-admin",
      password: "correct horse battery staple",
      domains: ["admin.apitoken.sale", "crm.apitoken.sale"],
      actorId: "legacy-main",
      reason: "bootstrap session regression account",
    }) as unknown as AccountResponse;
    const identity = await service.authenticatePassword({
      username: "session-admin",
      password: "correct horse battery staple",
      domain: "crm.apitoken.sale",
    });
    expect(identity?.sessionVersion).toMatch(/^[A-Za-z0-9_-]{43}$/);
    await expect(service.resolveSessionIdentity({
      accountId: created.account.id,
      domain: "crm.apitoken.sale",
      sessionVersion: identity!.sessionVersion,
    })).resolves.toMatchObject({ username: "session-admin" });

    await service.changePassword({
      accountId: created.account.id,
      password: "replacement horse battery staple",
      actorId: created.account.id,
      reason: "rotate persistent session credential",
    });
    await expect(service.resolveSessionIdentity({
      accountId: created.account.id,
      domain: "crm.apitoken.sale",
      sessionVersion: identity!.sessionVersion,
    })).resolves.toBeNull();

    const replacement = await service.authenticatePassword({
      username: "session-admin",
      password: "replacement horse battery staple",
      domain: "crm.apitoken.sale",
    });
    await service.setDomains({
      accountId: created.account.id,
      domains: ["admin.apitoken.sale"],
      actorId: created.account.id,
      reason: "remove CRM access",
    });
    await expect(service.resolveSessionIdentity({
      accountId: created.account.id,
      domain: "crm.apitoken.sale",
      sessionVersion: replacement!.sessionVersion,
    })).resolves.toBeNull();
  });

  it("prevents removal or disabling of the last active main-admin account", async () => {
    const first = await createMain(service);
    await expect(service.setDomains({
      accountId: first.account.id,
      domains: ["crm.apitoken.sale"],
      actorId: first.account.id,
      reason: "move operator",
    })).rejects.toBeInstanceOf(LastMainAdminError);
    await expect(service.setStatus({
      accountId: first.account.id,
      status: "disabled",
      actorId: first.account.id,
      reason: "leave company",
    })).rejects.toBeInstanceOf(LastMainAdminError);

    const second = await service.create({
      username: "backup-admin",
      password: "backup correct horse battery",
      domains: ["admin.apitoken.sale"],
      actorId: first.account.id,
      reason: "maintain emergency access",
    }) as unknown as AccountResponse;
    await expect(service.setStatus({
      accountId: first.account.id,
      status: "disabled",
      actorId: second.account.id,
      reason: "operator left company",
    })).resolves.toMatchObject({ account: { status: "disabled" } });
  });

  it("imports legacy panel and CRM bcrypt credentials atomically and upgrades them on login", async () => {
    const panelHash = await bcryptHash("panel legacy password", 4);
    const crmHash = await bcryptHash("crm legacy password", 4);
    const result = await service.importLegacy([
      {
        username: "Q",
        passwordHash: panelHash.replace(/^\$2b\$/, "$2y$"),
        domains: panelDomains,
      },
      { username: "Q_Sales", passwordHash: crmHash, domains: ["crm.apitoken.sale"] },
    ]);
    expect(result).toMatchObject({ accounts: 2, main_admin_accounts: 1, crm_accounts: 1 });

    await expect(service.authenticate({
      authorization: basic("Q", "panel legacy password"),
      domain: "content-studio.apitoken.sale",
    })).resolves.toMatchObject({ username: "Q" });
    const stored = await database.pool.query(`
      SELECT password_hash, password_changed_at FROM admin_accounts WHERE username = 'Q'
    `);
    expect(stored.rows[0].password_hash).toMatch(/^\$argon2id\$/);
    expect(stored.rows[0].password_changed_at).toBeInstanceOf(Date);
  });
});

const panelDomains: ManagedAdminDomain[] = [
  "admin.apitoken.sale",
  "admin.partners.apitoken.sale",
  "content-studio.apitoken.sale",
  "monitoring.apitoken.sale",
];

async function createMain(service: AdminAccountsService): Promise<AccountResponse> {
  return service.create({
    username: "main-admin",
    password: "correct horse battery staple",
    domains: ["admin.apitoken.sale"],
    actorId: "legacy-main",
    reason: "bootstrap managed admin",
  }) as unknown as Promise<AccountResponse>;
}

function basic(username: string, password: string): string {
  return `Basic ${Buffer.from(`${username}:${password}`).toString("base64")}`;
}

interface AccountResponse {
  account: { id: string; username: string; domains: ManagedAdminDomain[]; status: string };
}

interface AccountListResponse {
  accounts: Array<AccountResponse["account"]>;
}
