import {
  BadRequestException,
  Body,
  Controller,
  Get,
  Header,
  Headers,
  Post,
  Res,
  UnauthorizedException,
  UseGuards,
} from "@nestjs/common";
import { LegacyAdminImportConflictError, MANAGED_ADMIN_DOMAINS } from "@claude-api/db";
import { z } from "zod";
import {
  AdminAccountsService,
  isLegacyBcryptHash,
  isManagedAdminDomain,
} from "./admin-accounts.service.js";
import { AdminGuard } from "./admin.guard.js";

const legacyRowSchema = z.object({
  username: z.string().regex(/^[A-Za-z0-9._@-]{1,80}$/),
  password_hash: z.string().refine(isLegacyBcryptHash, "invalid legacy bcrypt hash"),
  domains: z.array(z.enum(MANAGED_ADMIN_DOMAINS)).min(1).max(MANAGED_ADMIN_DOMAINS.length),
}).strict();
const legacyImportSchema = z.object({ accounts: z.array(legacyRowSchema).min(2).max(100) }).strict();
interface ReplyLike { header(name: string, value: string | string[]): void }

@Controller("internal/admin-auth")
export class InternalAdminAuthController {
  constructor(private readonly accounts: AdminAccountsService) {}

  @Get("verify")
  @Header("Cache-Control", "no-store")
  async verify(
    @Headers("authorization") authorization: string | undefined,
    @Headers("x-admin-domain") domain: string | undefined,
    @Headers("host") host: string | undefined,
    @Res({ passthrough: true }) response: ReplyLike,
  ): Promise<Record<string, unknown>> {
    if (!isManagedAdminDomain(domain) || normalizedHost(host) !== domain) {
      throw new UnauthorizedException("admin authentication required");
    }
    const account = await this.accounts.authenticate({ authorization, domain });
    if (!account) {
      response.header("WWW-Authenticate", `Basic realm="${domain}", charset="UTF-8"`);
      throw new UnauthorizedException("admin authentication required");
    }
    response.header("X-Admin-Actor", account.username);
    response.header("X-Admin-Account-Id", account.id);
    return { authenticated: true };
  }

  @Post("legacy-import")
  @UseGuards(AdminGuard)
  @Header("Cache-Control", "no-store")
  async importLegacy(@Body() body: unknown): Promise<Record<string, unknown>> {
    const parsed = legacyImportSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    try {
      return await this.accounts.importLegacy(parsed.data.accounts.map((account) => ({
        username: account.username,
        passwordHash: account.password_hash,
        domains: account.domains,
      })));
    } catch (error) {
      if (error instanceof LegacyAdminImportConflictError) throw new BadRequestException(error.message);
      throw error;
    }
  }
}

function normalizedHost(value: string | undefined): string {
  return (value ?? "").trim().toLowerCase().replace(/:\d+$/, "");
}
