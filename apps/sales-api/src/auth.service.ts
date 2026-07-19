import { createHash, randomBytes } from "node:crypto";
import { Inject, Injectable } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import {
  clearPartnerRateLimit,
  consumePartnerRateLimit,
  createPartnerSession,
  createTelegramPartner,
  findTelegramPartner,
  getActiveInviteByCode,
  getActiveInviteByTelegramUsername,
  getPendingApplicationByTelegramId,
  submitApplication,
  resolvePartnerSession,
  revokePartnerSession,
  InvalidInviteError,
  ReferralCodeCollisionError,
  TelegramAlreadyRegisteredError,
  type Partner,
  type SalesDatabase,
} from "@claude-api/sales-db";
import type { Environment } from "./config.js";
import { SALES_DATABASE } from "./infrastructure.module.js";
import { normalizeTelegramUsername, verifyTelegramLogin, type TelegramLoginPayload } from "./telegram.js";

const REFERRAL_CODE_ALPHABET = "abcdefghijklmnopqrstuvwxyz234567";

export interface PartnerView {
  id: string;
  email: string | null;
  displayName: string | null;
  telegramUsername: string | null;
  telegramPhotoUrl: string | null;
  status: Partner["status"];
  referralCode: string;
  commissionBps: number;
  subCommissionBps: number;
  payoutMethod: string | null;
  payoutDetails: unknown;
  promoEnabled: boolean;
  promoMaxValueNano: string;
  promoMaxCount: number;
}

export interface PartnerSession {
  sessionId: string;
  token: string;
  expiresAt: Date;
  partner: PartnerView;
}

export class PartnerSuspendedError extends Error {}
export class AuthRateLimitedError extends Error {}
export class TelegramAuthDisabledError extends Error {}
export class TelegramSignatureError extends Error {}
export class NoAccountError extends Error {}
export class ApplicationPendingError extends Error {}
export { InvalidInviteError };

@Injectable()
export class AuthService {
  constructor(
    @Inject(SALES_DATABASE) private readonly database: SalesDatabase,
    private readonly config: ConfigService<Environment, true>,
  ) {}

  telegramBotUsername(): string | null {
    return this.config.get("TELEGRAM_BOT_USERNAME", { infer: true }) ?? null;
  }

  /**
   * Единственная точка входа: подписанный payload Telegram Login Widget.
   * Существующий telegram_id → сессия. Иначе аккаунт создаётся по инвайту:
   * явному (?invite=) или автоматически найденному по @username (онбординг
   * «просто войди»). Нет инвайта → NoAccountError (фронт предложит заявку),
   * а если заявка уже подана — ApplicationPendingError.
   */
  async telegramLogin(input: {
    payload: TelegramLoginPayload;
    inviteCode: string | null;
    userAgent: string | null;
    ipAddress: string | null;
  }): Promise<PartnerSession> {
    const botToken = this.config.get("TELEGRAM_BOT_TOKEN", { infer: true });
    if (!botToken || !this.telegramBotUsername()) {
      throw new TelegramAuthDisabledError("telegram login is not configured");
    }
    // Подпись проверяем ДО рейт-лимита по telegram_id: иначе неаутентифицированный
    // запрос с чужим id жёг бы бакет жертвы (таргетированный lockout).
    if (!verifyTelegramLogin(input.payload, botToken)) {
      throw new TelegramSignatureError("telegram login payload failed verification");
    }
    const keys = await this.enforceRateLimits("tg", input.payload.id, input.ipAddress, 20, 60, 900);
    const existing = await findTelegramPartner(this.database, input.payload.id);
    if (existing) {
      if (existing.status === "suspended") throw new PartnerSuspendedError("partner account is suspended");
      await clearPartnerRateLimit(this.database, keys);
      return this.issueSession(existing, input.userAgent, input.ipAddress);
    }
    let inviteCode = input.inviteCode;
    if (inviteCode) {
      // Ранняя проверка (читаемая ошибка до транзакции); авторитетная — в createTelegramPartner.
      const invite = await getActiveInviteByCode(this.database, inviteCode);
      if (!invite) throw new InvalidInviteError("invite code is invalid or expired");
    } else {
      const username = normalizeTelegramUsername(input.payload.username);
      const matched = username ? await getActiveInviteByTelegramUsername(this.database, username) : null;
      if (matched) {
        inviteCode = matched.code;
      } else {
        if (await getPendingApplicationByTelegramId(this.database, input.payload.id)) {
          throw new ApplicationPendingError("your application is being reviewed");
        }
        throw new NoAccountError("no partner account for this telegram");
      }
    }
    const partner = await this.createFromInvite({ payload: input.payload, inviteCode });
    await clearPartnerRateLimit(this.database, keys);
    return this.issueSession(partner, input.userAgent, input.ipAddress);
  }

  /** Заявка «с улицы»: подписанный payload + свободный текст. Идемпотентна по telegram_id. */
  async apply(input: {
    payload: TelegramLoginPayload;
    note: string | null;
    ipAddress: string | null;
  }): Promise<{ status: "applied" | "already_partner" }> {
    const botToken = this.config.get("TELEGRAM_BOT_TOKEN", { infer: true });
    if (!botToken) throw new TelegramAuthDisabledError("telegram login is not configured");
    // Подпись — до рейт-лимита (см. telegramLogin): защищает бакет жертвы от чужих запросов.
    if (!verifyTelegramLogin(input.payload, botToken)) {
      throw new TelegramSignatureError("telegram login payload failed verification");
    }
    await this.enforceRateLimits("tg-apply", input.payload.id, input.ipAddress, 10, 30, 3600);
    if (await findTelegramPartner(this.database, input.payload.id)) return { status: "already_partner" };
    await submitApplication(this.database, {
      telegramId: input.payload.id,
      telegramUsername: normalizeTelegramUsername(input.payload.username),
      displayName: [input.payload.first_name, input.payload.last_name].filter(Boolean).join(" ") || null,
      telegramPhotoUrl: input.payload.photo_url ?? null,
      note: input.note,
    });
    return { status: "applied" };
  }

  private async createFromInvite(input: {
    payload: TelegramLoginPayload;
    inviteCode: string;
  }): Promise<Partner> {
    const displayName = [input.payload.first_name, input.payload.last_name].filter(Boolean).join(" ") || null;
    for (let attempt = 0; ; attempt += 1) {
      try {
        return await createTelegramPartner(this.database, {
          telegramId: input.payload.id,
          telegramUsername: normalizeTelegramUsername(input.payload.username),
          telegramPhotoUrl: input.payload.photo_url ?? null,
          displayName,
          referralCode: generateCode(8),
          inviteCode: input.inviteCode,
          defaultCommissionBps: this.config.get("DEFAULT_COMMISSION_BPS", { infer: true }),
          defaultSubCommissionBps: this.config.get("DEFAULT_SUB_COMMISSION_BPS", { infer: true }),
        });
      } catch (error) {
        if (error instanceof ReferralCodeCollisionError && attempt < 5) continue;
        if (error instanceof TelegramAlreadyRegisteredError) {
          // Гонка двух логинов одного аккаунта: победителю уже создали партнёра.
          const partner = await findTelegramPartner(this.database, input.payload.id);
          if (partner && partner.status !== "suspended") return partner;
        }
        throw error;
      }
    }
  }

  async inviteInfo(code: string): Promise<{ telegramUsername: string | null } | null> {
    const invite = await getActiveInviteByCode(this.database, code);
    return invite ? { telegramUsername: invite.telegramUsername } : null;
  }

  async authenticate(token: string): Promise<{ sessionId: string; partner: PartnerView } | null> {
    if (!/^[A-Za-z0-9_-]{43}$/.test(token)) return null;
    const resolved = await resolvePartnerSession(this.database, tokenHash(token));
    return resolved ? { sessionId: resolved.sessionId, partner: partnerView(resolved.partner) } : null;
  }

  async logout(sessionId: string, partnerId: string): Promise<void> {
    await revokePartnerSession(this.database, sessionId, partnerId);
  }

  private async issueSession(partner: Partner, userAgent: string | null, ipAddress: string | null): Promise<PartnerSession> {
    const token = randomBytes(32).toString("base64url");
    const ttlSeconds = this.config.get("SALES_SESSION_TTL_SECONDS", { infer: true });
    const expiresAt = new Date(Date.now() + ttlSeconds * 1000);
    const sessionId = await createPartnerSession(this.database, {
      partnerId: partner.id,
      tokenHash: tokenHash(token),
      expiresAt,
      userAgent: userAgent?.slice(0, 500) ?? null,
      ipAddress: ipAddress?.slice(0, 100) ?? null,
    });
    return { sessionId, token, expiresAt, partner: partnerView(partner) };
  }

  private async enforceRateLimits(
    scope: string,
    subject: string,
    ipAddress: string | null,
    subjectMaximum: number,
    ipMaximum: number,
    windowSeconds: number,
  ): Promise<string[]> {
    const keys = [rateKey(`${scope}:subject:${subject.toLowerCase()}`), rateKey(`${scope}:ip:${ipAddress ?? "unknown"}`)];
    const [subjectAllowed, ipAllowed] = await Promise.all([
      consumePartnerRateLimit(this.database, { keyHash: keys[0]!, maximum: subjectMaximum, windowSeconds }),
      consumePartnerRateLimit(this.database, { keyHash: keys[1]!, maximum: ipMaximum, windowSeconds }),
    ]);
    if (!subjectAllowed || !ipAllowed) throw new AuthRateLimitedError("too many authentication attempts");
    return keys;
  }
}

export function partnerView(partner: Partner): PartnerView {
  return {
    id: partner.id,
    email: partner.email,
    displayName: partner.displayName,
    telegramUsername: partner.telegramUsername,
    telegramPhotoUrl: partner.telegramPhotoUrl,
    status: partner.status,
    referralCode: partner.referralCode,
    commissionBps: partner.commissionBps,
    subCommissionBps: partner.subCommissionBps,
    payoutMethod: partner.payoutMethod,
    payoutDetails: partner.payoutDetails,
    promoEnabled: partner.promoEnabled,
    promoMaxValueNano: partner.promoMaxValueNano.toString(),
    promoMaxCount: partner.promoMaxCount,
  };
}

export function generateCode(length: number): string {
  const bytes = randomBytes(length);
  let code = "";
  for (let index = 0; index < length; index += 1) {
    code += REFERRAL_CODE_ALPHABET[bytes[index]! % REFERRAL_CODE_ALPHABET.length];
  }
  return code;
}

function tokenHash(token: string): string {
  return createHash("sha256").update(token, "utf8").digest("hex");
}

function rateKey(value: string): string {
  return createHash("sha256").update(value, "utf8").digest("hex");
}
