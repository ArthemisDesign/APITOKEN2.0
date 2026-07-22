import type { Database } from "./client.js";

/**
 * Anti-abuse Layer 1 (signup bonus farming).
 *
 * Каждому юзеру соответствует ровно один signup_profile с сигналами регистрации
 * (IP, /24|/64-подсеть, UA, hash device-cookie). Welcome-бонус можно "заклеймить"
 * только если НИ ОДНО из условий кластера не совпадает с уже выданным бонусом:
 * одно устройство, одна подсеть или один канонический email — этого достаточно,
 * чтобы попасть в кластер и бонуса не получить. Гарантия атомарна: частичные
 * unique-индексы (WHERE bonus_granted) закрывают гонку конкурентных регистраций.
 */

/**
 * Welcome-бонус — только провайдерам, где регистрация ящика требует подтверждения по SMS
 * (проверено 2026-07: Gmail — телефон/QR с доверенного устройства; Yahoo/AOL — телефон
 * обязателен; Apple ID — номер + обычно устройство; Яндекс — телефон при полной регистрации;
 * Mail.ru — VK ID + телефон; Naver — корейский real-name). Осознанно ВНЕ списка: Proton
 * (регистрация без телефона by design), Microsoft (outlook/hotmail/live — главный канал
 * абуз-волны 2026-07-22), китайские freemail (qq/163/126 — телефонные фермы + та же волна),
 * temp- и кастомные домены. Аккаунт с прочим доменом работает нормально — просто без подарка.
 */
const BONUS_EMAIL_DOMAINS = new Set([
  "gmail.com", // googlemail canonicalized сюда же
  "yandex.ru", "yandex.com", "ya.ru",
  "mail.ru", "bk.ru", "inbox.ru", "list.ru",
  "icloud.com", "me.com", "mac.com",
  "yahoo.com", "ymail.com",
  "aol.com",
  "naver.com",
]);

/** true, если домен email в бонус-allowlist'е (сравнение по канонической форме). */
export function isBonusEligibleEmailDomain(email: string): boolean {
  const canonical = canonicalizeEmail(email);
  const at = canonical.lastIndexOf("@");
  return at !== -1 && BONUS_EMAIL_DOMAINS.has(canonical.slice(at + 1));
}

/** gmail/googlemail: точки и +alias схлопываются; прочие домены — только +alias. */
export function canonicalizeEmail(email: string): string {
  const lower = email.trim().toLowerCase();
  const at = lower.lastIndexOf("@");
  if (at === -1) return lower;
  let local = lower.slice(0, at);
  let domain = lower.slice(at + 1);
  const plus = local.indexOf("+");
  if (plus !== -1) local = local.slice(0, plus);
  if (domain === "googlemail.com") domain = "gmail.com";
  if (domain === "gmail.com") local = local.replaceAll(".", "");
  return `${local}@${domain}`;
}

/** IPv4 → /24, IPv6 → /64 (включая IPv4-mapped). Непарсибельный ввод → null. */
export function ipSubnetOf(ip: string | null): string | null {
  if (!ip) return null;
  const v4 = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.\d{1,3}$/.exec(ip);
  if (v4) return `${v4[1]}.${v4[2]}.${v4[3]}.0/24`;
  if (!ip.includes(":")) return null;
  const mapped = /^::ffff:(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})$/i.exec(ip);
  if (mapped) return ipSubnetOf(mapped[1]!);
  const groups = expandIpv6(ip.toLowerCase());
  return groups ? `${groups.slice(0, 4).join(":")}::/64` : null;
}

function expandIpv6(ip: string): string[] | null {
  if ((ip.match(/::/g) ?? []).length > 1) return null;
  const [head = "", tail = ""] = ip.split("::");
  const headParts = head ? head.split(":") : [];
  const tailParts = tail ? tail.split(":") : [];
  const missing = 8 - headParts.length - tailParts.length;
  const parts = ip.includes("::")
    ? [...headParts, ...Array<string>(Math.max(missing, 0)).fill("0"), ...tailParts]
    : headParts;
  if (parts.length !== 8 || parts.some((p) => !/^[0-9a-f]{1,4}$/.test(p))) return null;
  return parts;
}

export interface SignupProfileInput {
  userId: string;
  emailCanonical: string;
  ipAddress: string | null;
  ipSubnet: string | null;
  userAgent: string | null;
  deviceHash: string | null;
}

/**
 * Идемпотентная запись профиля. Существующий профиль (в т.ч. backfill) дополняется
 * только пустыми сигналами — первый зафиксированный IP/девайс не перезаписывается.
 */
export async function upsertSignupProfile(
  database: Database,
  input: SignupProfileInput,
): Promise<{ bonusGranted: boolean; flaggedReason: string | null }> {
  const result = await database.pool.query<{ bonus_granted: boolean; flagged_reason: string | null }>(`
    INSERT INTO signup_profiles (user_id, email_canonical, ip_address, ip_subnet, user_agent, device_hash)
    VALUES ($1, $2, $3, $4, $5, $6)
    ON CONFLICT (user_id) DO UPDATE SET
      ip_address = COALESCE(signup_profiles.ip_address, EXCLUDED.ip_address),
      ip_subnet = COALESCE(signup_profiles.ip_subnet, EXCLUDED.ip_subnet),
      user_agent = COALESCE(signup_profiles.user_agent, EXCLUDED.user_agent),
      device_hash = COALESCE(signup_profiles.device_hash, EXCLUDED.device_hash)
    RETURNING bonus_granted, flagged_reason
  `, [input.userId, input.emailCanonical, input.ipAddress, input.ipSubnet,
      input.userAgent?.slice(0, 500) ?? null, input.deviceHash]);
  return {
    bonusGranted: result.rows[0]?.bonus_granted ?? false,
    flaggedReason: result.rows[0]?.flagged_reason ?? null,
  };
}

export type SignupBonusClaim =
  | { claimed: true }
  | { claimed: false; reason: "already-granted" | "duplicate-device" | "duplicate-subnet" | "duplicate-email" };

/**
 * Пытается атомарно заклеймить бонус. Совпадение устройства/подсети/email с уже
 * выданным бонусом бьётся о частичный unique-индекс → отказ + flagged_reason.
 */
export async function claimSignupBonus(database: Database, userId: string): Promise<SignupBonusClaim> {
  try {
    // Флагнутый профиль (velocity/домен/дубль) НЕ клеймит никогда — иначе абузер пережидает
    // окно velocity и добирает бонусы повторным входом.
    const result = await database.pool.query(
      "UPDATE signup_profiles SET bonus_granted = true WHERE user_id = $1 AND NOT bonus_granted AND flagged_reason IS NULL",
      [userId],
    );
    return result.rowCount === 1 ? { claimed: true } : { claimed: false, reason: "already-granted" };
  } catch (error) {
    const reason = duplicateReason(error);
    if (!reason) throw error;
    await flagSignupProfile(database, userId, reason);
    return { claimed: false, reason };
  }
}

/** Откат клейма, если зачисление в движок не удалось (повторный вход попробует снова). */
export async function releaseSignupBonus(database: Database, userId: string): Promise<void> {
  await database.pool.query("UPDATE signup_profiles SET bonus_granted = false WHERE user_id = $1", [userId]);
}

export async function flagSignupProfile(database: Database, userId: string, reason: string): Promise<void> {
  await database.pool.query(
    "UPDATE signup_profiles SET flagged_reason = COALESCE(flagged_reason, $2) WHERE user_id = $1",
    [userId, reason],
  );
}

/** Сколько регистраций пришло из подсети за окно — сигнал волновой атаки. */
export async function countRecentSubnetSignups(
  database: Database,
  subnet: string,
  windowSeconds: number,
): Promise<number> {
  const result = await database.pool.query<{ n: string }>(
    "SELECT count(*) AS n FROM signup_profiles WHERE ip_subnet = $1 AND created_at > now() - ($2 * interval '1 second')",
    [subnet, windowSeconds],
  );
  return Number(result.rows[0]?.n ?? 0);
}

/** Журнал «этот браузер видел эти аккаунты» — связывает мульти-аккаунты навсегда. */
export async function recordDeviceSighting(database: Database, deviceHash: string, userId: string): Promise<void> {
  await database.pool.query(`
    INSERT INTO device_sightings (device_hash, user_id)
    VALUES ($1, $2)
    ON CONFLICT (device_hash, user_id) DO UPDATE SET last_seen_at = now()
  `, [deviceHash, userId]);
}

function duplicateReason(error: unknown): "duplicate-device" | "duplicate-subnet" | "duplicate-email" | null {
  if (typeof error !== "object" || error === null) return null;
  const pg = error as { code?: string; constraint?: string };
  if (pg.code !== "23505") return null;
  switch (pg.constraint) {
    case "signup_bonus_device_uidx": return "duplicate-device";
    case "signup_bonus_subnet_uidx": return "duplicate-subnet";
    case "signup_bonus_email_uidx": return "duplicate-email";
    default: return null;
  }
}
