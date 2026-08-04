import { createHash } from "node:crypto";
import { Logger } from "@nestjs/common";
import { B2C_SIGNUP_BONUS_BALANCE_NANO } from "@claude-api/contracts";
import {
  canonicalizeEmail,
  claimSignupBonus,
  countRecentSubnetSignups,
  flagSignupProfile,
  ipSubnetOf,
  isBonusEligibleEmailDomain,
  releaseSignupBonus,
  upsertSignupProfile,
  type Database,
} from "@claude-api/db";
import type { EngineClient } from "@claude-api/engine-client";

const logger = new Logger("SignupBonus");

// Волна регистраций из одной подсети: сверх этого числа за окно бонус не выдаётся (флаг).
const SUBNET_SIGNUP_MAXIMUM = 3;
const SUBNET_SIGNUP_WINDOW_SECONDS = 86_400;

export interface SignupBonusMeta {
  userAgent?: string | null;
  ipAddress?: string | null;
  deviceToken?: string | null;
}

/**
 * Единая точка выдачи welcome-бонуса с антифрод-гейтом. Антифрод-профиль и флаги фиксируются
 * ВСЕГДА (первый зафиксированный сигнал не перезаписывается), а клейм происходит только против
 * engine-аккаунта, который УЖЕ active по свежему чтению из БД: in-memory статус после
 * managed-provisioning остаётся pending до асинхронного ACK worker'а, и гейт по нему терял
 * бонус вместе с профилем. Вызывается из OAuth-входа и из AccountService.ensureEngineAccount —
 * клейм атомарен (частичные unique-индексы), зачисление идемпотентно по ref, поэтому
 * параллельные и повторные вызовы безопасны. В кластер достаточно попасть ЛЮБЫМ одним
 * признаком: то же устройство (device-cookie), та же /24|/64 подсеть или тот же канонический
 * email, что у уже выданного бонуса, — тогда аккаунт создаётся, но бонус не выдаётся.
 */
export async function settleSignupBonus(
  database: Database,
  engine: EngineClient,
  input: { userId: string; email: string; customerType: "b2c" | "b2b"; meta?: SignupBonusMeta },
): Promise<void> {
  if (input.customerType !== "b2c") return;
  let profile: { bonusGranted: boolean; flaggedReason: string | null };
  try {
    profile = await upsertSignupProfile(database, {
      userId: input.userId,
      emailCanonical: canonicalizeEmail(input.email),
      ipAddress: input.meta?.ipAddress?.slice(0, 100) ?? null,
      ipSubnet: ipSubnetOf(input.meta?.ipAddress ?? null),
      userAgent: input.meta?.userAgent ?? null,
      deviceHash: deviceHashOf(input.meta?.deviceToken ?? null),
    });
  } catch (error) {
    // В сомнительной ситуации бонус НЕ выдаём (fail-closed), но сбой наблюдаем в логах.
    logger.warn(`signup profile recording failed for user ${input.userId}: ${errorMessage(error)}`);
    return;
  }
  if (profile.bonusGranted || profile.flaggedReason !== null) return;
  // Подарок — только популярным почтовым провайдерам; GitHub OAuth пускает любой
  // верифицированный ящик, включая одноразовые домены — эту дыру закрывает allowlist.
  if (!isBonusEligibleEmailDomain(input.email)) {
    await flagSignupProfile(database, input.userId, "email-domain");
    return;
  }
  const subnet = ipSubnetOf(input.meta?.ipAddress ?? null);
  if (subnet && await countRecentSubnetSignups(database, subnet, SUBNET_SIGNUP_WINDOW_SECONDS) > SUBNET_SIGNUP_MAXIMUM) {
    await flagSignupProfile(database, input.userId, "subnet-velocity");
    return;
  }
  // Клейм — только против подтверждённого active аккаунта. Аккаунт ещё pending (worker не
  // подтвердил managed-политику) → профиль сохранён, клейм повторится при следующем вызове.
  const account = await database.pool.query<{ engine_account_id: string | null }>(`
    SELECT engine_account_id
    FROM engine_accounts
    WHERE user_id = $1 AND status = 'active'
  `, [input.userId]);
  const engineAccountId = account.rows[0]?.engine_account_id;
  if (!engineAccountId) return;
  const claim = await claimSignupBonus(
    database,
    input.userId,
    B2C_SIGNUP_BONUS_BALANCE_NANO,
  );
  if (!claim.claimed) return;
  try {
    await engine.creditAccount(
      engineAccountId,
      B2C_SIGNUP_BONUS_BALANCE_NANO,
      `signup-bonus:${input.userId}`,
    );
  } catch (error) {
    // зачисление идемпотентно по ref — освобождаем клейм, следующий вызов попробует снова
    await releaseSignupBonus(database, input.userId);
    throw error;
  }
}

// Device-cookie: 32 байта base64url (43 символа). Иной формат → сигнала нет (null).
export function deviceHashOf(token: string | null): string | null {
  if (!token || !/^[A-Za-z0-9_-]{43}$/.test(token)) return null;
  return createHash("sha256").update(token, "utf8").digest("hex");
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
