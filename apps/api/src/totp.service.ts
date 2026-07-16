import { BadRequestException, ConflictException, Inject, Injectable, UnauthorizedException } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { authenticator } from "otplib";
import QRCode from "qrcode";
import {
  clearUserTotp,
  decodeAuthEncryptionKey,
  decryptAuthToken,
  enableUserTotp,
  encryptAuthToken,
  getUserTotp,
  setUserTotpPending,
  type Database,
} from "@claude-api/db";
import type { Environment } from "./config.js";
import { DATABASE } from "./infrastructure.module.js";

const ISSUER = "apiToken.sale";

// TOTP (Google Authenticator) 2FA. Секрет живёт в users.totp_secret ЗАШИФРОВАННЫМ (AES-GCM, тот же
// ключ AUTH_TOKEN_ENCRYPTION_KEY, что и почтовые токены). Открытый секрет существует только в памяти
// на момент setup/verify. Гейт на выпуск ключей зовёт verify().
@Injectable()
export class TotpService {
  constructor(
    @Inject(DATABASE) private readonly database: Database,
    private readonly config: ConfigService<Environment, true>,
  ) {}

  private key(): Buffer {
    return decodeAuthEncryptionKey(this.config.get("AUTH_TOKEN_ENCRYPTION_KEY", { infer: true }));
  }

  // Завести (или перевыпустить) секрет в статусе pending и вернуть данные для приложения-аутентификатора.
  async setup(userId: string, email: string): Promise<{ otpauthUri: string; secret: string; qrDataUrl: string }> {
    const existing = await getUserTotp(this.database, userId);
    if (existing?.enabled) throw new ConflictException("two-factor authentication is already enabled");
    const secret = authenticator.generateSecret();
    await setUserTotpPending(this.database, userId, encryptAuthToken(secret, this.key()));
    const otpauthUri = authenticator.keyuri(email, ISSUER, secret);
    const qrDataUrl = await QRCode.toDataURL(otpauthUri, { margin: 1, width: 232 });
    return { otpauthUri, secret, qrDataUrl };
  }

  // Подтвердить код из приложения и включить 2FA.
  async enable(userId: string, code: string): Promise<void> {
    const row = await getUserTotp(this.database, userId);
    if (!row?.secret) throw new BadRequestException("start two-factor setup first");
    if (row.enabled) return;
    if (!this.check(row.secret, code)) throw new UnauthorizedException("invalid authentication code");
    await enableUserTotp(this.database, userId);
  }

  // Снять 2FA (требует действующий код).
  async disable(userId: string, code: string): Promise<void> {
    const row = await getUserTotp(this.database, userId);
    if (!row?.enabled || !row.secret) return;
    if (!this.check(row.secret, code)) throw new UnauthorizedException("invalid authentication code");
    await clearUserTotp(this.database, userId);
  }

  // Проверка кода для гейта на выпуск ключа. Никогда не бросает — возвращает true/false.
  async verify(userId: string, code: string): Promise<boolean> {
    const row = await getUserTotp(this.database, userId);
    if (!row?.enabled || !row.secret) return false;
    return this.check(row.secret, code);
  }

  private check(encryptedSecret: string, code: string): boolean {
    let secret: string;
    try {
      secret = decryptAuthToken(encryptedSecret, this.key());
    } catch {
      return false;
    }
    try {
      return authenticator.verify({ token: code, secret });
    } catch {
      return false;
    }
  }
}
