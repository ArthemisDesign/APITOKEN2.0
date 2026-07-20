import {
  BadRequestException,
  Body,
  ConflictException,
  Controller,
  Get,
  Header,
  Headers,
  HttpException,
  NotFoundException,
  Param,
  Patch,
  Post,
  Query,
  UseGuards,
} from "@nestjs/common";
import {
  LastMainAdminError,
  MANAGED_ADMIN_DOMAINS,
  ManagedAdminConflictError,
  ManagedAdminNotFoundError,
} from "@claude-api/db";
import { z } from "zod";
import { AdminAccountsService, isManagedAdminDomain } from "./admin-accounts.service.js";
import { AdminGuard } from "./admin.guard.js";

const usernameSchema = z.string().trim().regex(/^[A-Za-z0-9._@-]{1,80}$/);
const passwordSchema = z.string().min(12).max(200);
const reasonSchema = z.string().trim().min(3).max(300);
const domainSchema = z.enum(MANAGED_ADMIN_DOMAINS);
const domainsSchema = z.array(domainSchema).min(1).max(MANAGED_ADMIN_DOMAINS.length)
  .refine((domains) => new Set(domains).size === domains.length, "domains must be unique");
const createSchema = z.object({
  username: usernameSchema,
  password: passwordSchema,
  domains: domainsSchema,
  reason: reasonSchema,
}).strict();
const passwordChangeSchema = z.object({ password: passwordSchema, reason: reasonSchema }).strict();
const domainsChangeSchema = z.object({ domains: domainsSchema, reason: reasonSchema }).strict();
const statusChangeSchema = z.object({ status: z.enum(["active", "disabled"]), reason: reasonSchema }).strict();
const uuidSchema = z.string().uuid();

@Controller("admin/admin-accounts")
@UseGuards(AdminGuard)
export class AdminAccountsController {
  constructor(private readonly accounts: AdminAccountsService) {}

  @Get("domains")
  @Header("Cache-Control", "no-store")
  domains(): Record<string, unknown> {
    return this.accounts.domains();
  }

  @Get()
  @Header("Cache-Control", "no-store")
  list(
    @Query("domain") domain?: string,
    @Headers("x-admin-account-id") currentAccountId?: string,
  ): Promise<Record<string, unknown>> {
    if (domain !== undefined && !isManagedAdminDomain(domain)) {
      throw new BadRequestException("unknown managed admin domain");
    }
    return this.accounts.list(domain, uuidSchema.safeParse(currentAccountId).success ? currentAccountId : undefined);
  }

  @Post()
  @Header("Cache-Control", "no-store")
  async create(
    @Body() body: unknown,
    @Headers("x-admin-account-id") accountId?: string,
    @Headers("x-admin-actor") actor?: string,
  ): Promise<Record<string, unknown>> {
    const parsed = createSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    return this.mapErrors(() => this.accounts.create({
      ...parsed.data,
      actorId: adminActor(accountId, actor),
    }));
  }

  @Patch(":id/password")
  @Header("Cache-Control", "no-store")
  async changePassword(
    @Param("id") id: string,
    @Body() body: unknown,
    @Headers("x-admin-account-id") accountId?: string,
    @Headers("x-admin-actor") actor?: string,
  ): Promise<Record<string, unknown>> {
    assertAccountId(id);
    const parsed = passwordChangeSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    return this.mapErrors(() => this.accounts.changePassword({
      accountId: id,
      ...parsed.data,
      actorId: adminActor(accountId, actor),
    }));
  }

  @Patch(":id/domains")
  @Header("Cache-Control", "no-store")
  async setDomains(
    @Param("id") id: string,
    @Body() body: unknown,
    @Headers("x-admin-account-id") accountId?: string,
    @Headers("x-admin-actor") actor?: string,
  ): Promise<Record<string, unknown>> {
    assertAccountId(id);
    const parsed = domainsChangeSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    return this.mapErrors(() => this.accounts.setDomains({
      accountId: id,
      ...parsed.data,
      actorId: adminActor(accountId, actor),
    }));
  }

  @Patch(":id/status")
  @Header("Cache-Control", "no-store")
  async setStatus(
    @Param("id") id: string,
    @Body() body: unknown,
    @Headers("x-admin-account-id") accountId?: string,
    @Headers("x-admin-actor") actor?: string,
  ): Promise<Record<string, unknown>> {
    assertAccountId(id);
    const parsed = statusChangeSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    return this.mapErrors(() => this.accounts.setStatus({
      accountId: id,
      ...parsed.data,
      actorId: adminActor(accountId, actor),
    }));
  }

  private async mapErrors(action: () => Promise<Record<string, unknown>>): Promise<Record<string, unknown>> {
    try {
      return await action();
    } catch (error) {
      if (error instanceof ManagedAdminConflictError) throw new ConflictException(error.message);
      if (error instanceof ManagedAdminNotFoundError) throw new NotFoundException(error.message);
      if (error instanceof LastMainAdminError) throw new HttpException(error.message, 409);
      throw error;
    }
  }
}

function assertAccountId(id: string): void {
  if (!uuidSchema.safeParse(id).success) throw new BadRequestException("admin account ID must be a UUID");
}

function adminActor(accountId: string | undefined, username: string | undefined): string {
  if (accountId && uuidSchema.safeParse(accountId).success) return accountId;
  const normalized = username?.trim() ?? "";
  return /^[A-Za-z0-9._@-]{1,80}$/.test(normalized) ? normalized : "unknown";
}
