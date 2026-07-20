import { Inject, Injectable, Logger } from "@nestjs/common";
import {
  changeManagedAdminPassword,
  createManagedAdminAccount,
  findManagedAdminCredential,
  importLegacyAdminAccounts,
  listManagedAdminAccounts,
  MANAGED_ADMIN_DOMAINS,
  setManagedAdminDomains,
  setManagedAdminStatus,
  upgradeLegacyAdminPasswordHash,
  type Database,
  type ManagedAdminAccount,
  type ManagedAdminDomain,
  type ManagedAdminStatus,
} from "@claude-api/db";
import { argon2id, hash, verify } from "argon2";
import { compare } from "bcryptjs";
import { DATABASE } from "./infrastructure.module.js";

const dummyHash = hash("not-a-real-managed-admin-password", passwordHashOptions());

export const MANAGED_ADMIN_DOMAIN_DETAILS = [
  { domain: "admin.apitoken.sale", label: "Main admin" },
  { domain: "admin.partners.apitoken.sale", label: "Partners admin" },
  { domain: "crm.apitoken.sale", label: "CRM" },
  { domain: "content-studio.apitoken.sale", label: "Content Studio" },
] as const satisfies readonly { domain: ManagedAdminDomain; label: string }[];

@Injectable()
export class AdminAccountsService {
  private readonly logger = new Logger(AdminAccountsService.name);

  constructor(@Inject(DATABASE) private readonly database: Database) {}

  domains(): Record<string, unknown> {
    return {
      domains: MANAGED_ADMIN_DOMAIN_DETAILS,
      external_domains: [{
        domain: "support.apitoken.sale",
        label: "Support",
        account_system: "Chatwoot",
      }],
    };
  }

  async list(domain?: ManagedAdminDomain, currentAccountId?: string): Promise<Record<string, unknown>> {
    const accounts = await listManagedAdminAccounts(this.database, domain);
    return {
      domain: domain ?? null,
      current_account_id: currentAccountId ?? null,
      accounts: accounts.map(serializeAccount),
    };
  }

  async create(input: {
    username: string;
    password: string;
    domains: readonly ManagedAdminDomain[];
    actorId: string;
    reason: string;
  }): Promise<Record<string, unknown>> {
    const account = await createManagedAdminAccount(this.database, {
      ...input,
      passwordHash: await hash(input.password, passwordHashOptions()),
    });
    return { account: serializeAccount(account) };
  }

  async changePassword(input: {
    accountId: string;
    password: string;
    actorId: string;
    reason: string;
  }): Promise<Record<string, unknown>> {
    const startedAt = Date.now();
    try {
      const account = await changeManagedAdminPassword(this.database, {
        ...input,
        passwordHash: await hash(input.password, passwordHashOptions()),
      });
      this.logger.log(JSON.stringify({
        event: "admin_account.password_changed",
        target_account_id: input.accountId,
        actor_id: input.actorId,
        changed_self: input.accountId === input.actorId,
        duration_ms: Date.now() - startedAt,
      }));
      return {
        account: serializeAccount(account),
        changed_self: input.accountId === input.actorId,
      };
    } catch (error) {
      this.logger.error(JSON.stringify({
        event: "admin_account.password_change_failed",
        target_account_id: input.accountId,
        actor_id: input.actorId,
        error: error instanceof Error ? error.name : "UnknownError",
        duration_ms: Date.now() - startedAt,
      }));
      throw error;
    }
  }

  async setDomains(input: {
    accountId: string;
    domains: readonly ManagedAdminDomain[];
    actorId: string;
    reason: string;
  }): Promise<Record<string, unknown>> {
    const account = await setManagedAdminDomains(this.database, input);
    return { account: serializeAccount(account) };
  }

  async setStatus(input: {
    accountId: string;
    status: ManagedAdminStatus;
    actorId: string;
    reason: string;
  }): Promise<Record<string, unknown>> {
    const account = await setManagedAdminStatus(this.database, input);
    return { account: serializeAccount(account) };
  }

  async authenticate(input: {
    authorization: string | undefined;
    domain: ManagedAdminDomain;
  }): Promise<{ id: string; username: string } | null> {
    const basic = parseBasicAuthorization(input.authorization);
    const account = basic
      ? await findManagedAdminCredential(this.database, { username: basic.username, domain: input.domain })
      : null;
    const candidateHash = account?.passwordHash ?? await dummyHash;
    let valid = false;
    try {
      valid = basic ? await verifyPassword(candidateHash, basic.password) : false;
    } catch {
      valid = false;
    }
    if (!basic || !account || !valid || account.status !== "active") return null;
    if (isBcryptHash(account.passwordHash)) {
      const upgraded = await hash(basic.password, passwordHashOptions());
      await upgradeLegacyAdminPasswordHash(this.database, {
        accountId: account.id,
        previousHash: account.passwordHash,
        passwordHash: upgraded,
      });
    }
    return { id: account.id, username: account.username };
  }

  async importLegacy(input: readonly {
    username: string;
    passwordHash: string;
    domains: readonly ManagedAdminDomain[];
  }[]): Promise<Record<string, unknown>> {
    const result = await importLegacyAdminAccounts(this.database, input);
    return {
      imported: result.imported,
      accounts: result.accounts,
      main_admin_accounts: result.mainAdminAccounts,
      crm_accounts: result.crmAccounts,
    };
  }
}

export function isManagedAdminDomain(value: string | undefined): value is ManagedAdminDomain {
  return typeof value === "string" && (MANAGED_ADMIN_DOMAINS as readonly string[]).includes(value);
}

export function isLegacyBcryptHash(value: string): boolean {
  return /^\$2[aby]\$\d\d\$[./A-Za-z0-9]{53}$/.test(value);
}

function isBcryptHash(value: string): boolean {
  return /^\$2[aby]\$/.test(value);
}

async function verifyPassword(passwordHash: string, password: string): Promise<boolean> {
  if (isBcryptHash(passwordHash)) return compare(password, passwordHash);
  if (passwordHash.startsWith("$argon2")) return verify(passwordHash, password);
  return false;
}

function passwordHashOptions(): Parameters<typeof hash>[1] {
  return { type: argon2id, memoryCost: 19_456, timeCost: 2, parallelism: 1 };
}

function parseBasicAuthorization(value: string | undefined): { username: string; password: string } | null {
  const match = /^Basic ([A-Za-z0-9+/]+={0,2})$/.exec(value ?? "");
  if (!match) return null;
  let decoded: string;
  try {
    decoded = Buffer.from(match[1]!, "base64").toString("utf8");
  } catch {
    return null;
  }
  const separator = decoded.indexOf(":");
  if (separator < 1) return null;
  const username = decoded.slice(0, separator);
  if (!/^[A-Za-z0-9._@-]{1,80}$/.test(username)) return null;
  return { username, password: decoded.slice(separator + 1) };
}

function serializeAccount(account: ManagedAdminAccount): Record<string, unknown> {
  return {
    id: account.id,
    username: account.username,
    status: account.status,
    domains: account.domains,
    password_changed_at: account.passwordChangedAt?.toISOString() ?? null,
    created_at: account.createdAt.toISOString(),
    updated_at: account.updatedAt.toISOString(),
  };
}
