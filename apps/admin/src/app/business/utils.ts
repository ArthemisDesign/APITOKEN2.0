// Чистая логика вкладки «B2B», портированная 1:1 из business()/bindBusiness()
// в crates/server/src/admin-panel.js (строки 892-938). Вынесена из page.tsx,
// чтобы тестировалась в node-окружении vitest без рендера.

// Причина всех ручных действий из панели (PANEL_REASON в легаси).
export const PANEL_REASON = "ручное действие из админ-панели";

// Ключ sessionStorage для безопасного повтора создания инвайта (легаси).
export const INVITE_PENDING_KEY = "business-invite-pending";
// Префикс ключа безопасного повтора переотправки: 'business-invite-resend:<id>'.
export const RESEND_PENDING_PREFIX = "business-invite-resend:";

// GET /admin/users?customer_type=b2b — элемент списка B2B-клиентов.
export interface BusinessUser {
  id: string;
  email: string;
  multiplier_bp?: number | null;
  balance_usd?: number | string | null;
  engine_account_status?: string | null;
  pricing_sync_status?: string | null;
  pricing_sync_error?: string | null;
}

// GET /admin/business-invites — элемент списка инвайтов.
export interface BusinessInvite {
  id: string;
  email?: string | null;
  discount_percent?: number | null;
  consumed_at?: string | null;
  revoked_at?: string | null;
  expires_at?: string | null;
  delivery_status?: string | null;
  delivery_error?: string | null;
  policy_version?: number | null;
  policy_digest?: string | null;
  policy_rule_count?: number | null;
}

export interface BusinessUsersPage {
  users?: BusinessUser[];
  total?: number;
}

export interface BusinessInvitesPage {
  invites?: BusinessInvite[];
}

// Договорная скидка из multiplier_bp: 100 - bp/100 (bp 3000 → 70%).
// null на входе → null (страница покажет «—» вместо NaN%).
export function discountFromMultiplierBp(bp: number | null | undefined): number | null {
  return bp == null ? null : 100 - bp / 100;
}

// Целое число в диапазоне [min, max] или null — валидация скидки/срока,
// как Number.isInteger-проверки в легаси.
export function parseBoundedInteger(raw: string, min: number, max: number): number | null {
  const value = Number(raw);
  if (!Number.isInteger(value) || value < min || value > max) return null;
  return value;
}

export type InviteState = { label: string; kind: "ok" | "warn" | "bad" };

// Статус инвайта: использован / отозван / истёк / активен (порядок проверок — как в легаси).
export function inviteState(invite: BusinessInvite, now: Date = new Date()): InviteState {
  if (invite.consumed_at) return { label: "использован", kind: "ok" };
  if (invite.revoked_at) return { label: "отозван", kind: "bad" };
  if (!invite.expires_at || new Date(invite.expires_at) < now) return { label: "истёк", kind: "bad" };
  return { label: "активен", kind: "warn" };
}

// Действия (копировать/отправить заново/отозвать) — только для живого инвайта.
export function isInviteActive(invite: BusinessInvite, now: Date = new Date()): boolean {
  return !invite.consumed_at && !invite.revoked_at && Boolean(invite.expires_at) && new Date(invite.expires_at!) > now;
}

export type DeliveryPill = { label: string; kind: "ok" | "warn" | "bad" | "info" };

// Доставка письма: sent → ok, failed → bad, остальное → warn;
// инвайт без email — «copy only» (info), статус доставки не показывается.
export function deliveryPill(invite: BusinessInvite): DeliveryPill {
  if (!invite.email) return { label: "copy only", kind: "info" };
  const status = invite.delivery_status ?? "—";
  return { label: status, kind: status === "sent" ? "ok" : status === "failed" ? "bad" : "warn" };
}

// Идемпотентность как в легаси: перед запросом ключ сохраняется в sessionStorage
// вместе с подписью параметров; если предыдущая попытка с той же подписью упала
// (уже по сети), повтор уходит с тем же ключом — бэкенд отдаёт результат первой
// попытки вместо дубликата. После успеха ключ стирается вызывающим кодом.
export function reuseIdempotencyKey(storageKey: string, signature: string): string {
  let idempotencyKey = crypto.randomUUID();
  try {
    const pending = JSON.parse(sessionStorage.getItem(storageKey) ?? "null") as {
      signature?: unknown;
      idempotencyKey?: unknown;
    } | null;
    if (pending?.signature === signature && typeof pending.idempotencyKey === "string") {
      idempotencyKey = pending.idempotencyKey;
    }
  } catch {
    // Битая запись/недоступное хранилище — просто новый ключ.
  }
  try {
    sessionStorage.setItem(storageKey, JSON.stringify({ signature, idempotencyKey }));
  } catch {
    // Легаси тоже продолжал бы работу; без записи повтор просто создаст новый ключ.
  }
  return idempotencyKey;
}

// copyText из легаси: navigator.clipboard, иначе скрытый textarea + execCommand.
export async function copyText(value: string): Promise<void> {
  if (navigator.clipboard?.writeText) return navigator.clipboard.writeText(value);
  const input = document.createElement("textarea");
  input.value = value;
  input.style.position = "fixed";
  input.style.opacity = "0";
  document.body.appendChild(input);
  input.select();
  const copied = document.execCommand("copy");
  input.remove();
  if (!copied) throw new Error("Не удалось скопировать ссылку");
}
