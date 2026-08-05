// Типы payload'а и чистые хелперы страницы OpenKeys — порт 1:1 секции
// openkeys()/bindOpenkeys() из crates/server/src/admin-panel.js (строки 606-635).
// Вынесено из page.tsx, чтобы логика тестировалась в node-окружении vitest.

export const PAGE_LIMIT = 50;

export type OpenkeysBatch = {
  id?: string;
  label?: string;
  createdBy?: string;
  createdAt?: string;
};

export type OpenkeysRow = {
  id?: string;
  keyMasked?: string;
  engineAccountId?: string;
  apiType?: string;
  batchId?: string;
  batchLabel?: string;
  createdBy?: string;
  enabled?: boolean;
  status?: string;
  usageState?: string;
  usagePercent?: number | null;
  spentNano?: string | null;
  remainingNano?: string | null;
  faceValueNano?: string;
  deliveredAt?: string | null;
  createdAt?: string;
  viewUrl?: string;
};

export type OpenkeysSummary = {
  active?: number;
  disabled?: number;
  used?: number;
  unused?: number;
  exhausted?: number;
  spentNano?: string;
  remainingNano?: string;
};

export type OpenkeysResponse = {
  rows?: OpenkeysRow[];
  batches?: OpenkeysBatch[];
  total?: number;
  summary?: OpenkeysSummary;
  truncated?: boolean;
};

// Состояние фильтров и пейджинга (openkeysPage в легаси).
export type OpenkeysQuery = {
  offset: number;
  q: string;
  batch: string;
  status: string;
  usage: string;
};

// Путь каталога с фильтрами (admin-panel.js:607-609): limit/offset — всегда,
// q/batch/status/usage — только когда заданы.
export function buildKeysPath(query: OpenkeysQuery): string {
  const params = new URLSearchParams({
    limit: String(PAGE_LIMIT),
    offset: String(query.offset),
  });
  if (query.q) params.set("q", query.q);
  if (query.batch) params.set("batch", query.batch);
  if (query.status) params.set("status", query.status);
  if (query.usage) params.set("usage", query.usage);
  return "/openkeys-admin/keys?" + params.toString();
}

// Если offset уехал за пределы total (фильтры сузили выдачу), легаси молча
// откатывается на последнюю страницу и перезапрашивает (admin-panel.js:612).
export function clampOffset(offset: number, total: number): number {
  if (total <= 0 || offset < total) return offset;
  return Math.max(0, Math.floor((total - 1) / PAGE_LIMIT) * PAGE_LIMIT);
}

// Подписи использования в таблице (usageLabel, admin-panel.js:613).
export const USAGE_LABELS: Record<string, string> = {
  unused: "не использовался",
  used: "используется",
  exhausted: "исчерпан",
  unavailable: "нет live-данных",
};

// okTypeLabel (admin-panel.js:407): openai → OpenAI, всё остальное → Claude.
export function okTypeLabel(type: string | null | undefined): string {
  return type === "openai" ? "OpenAI" : "Claude";
}

// --- Админы-издатели (bulk-управление ключами одного created_by) ---------------

export const SELLERS_PATH = "/openkeys-admin/sellers";

export type SellerAction = "pause" | "resume" | "revoke";

export type OpenkeysSeller = {
  createdBy?: string;
  batches?: number;
  keys?: number;
  active?: number;
  disabled?: number;
  delivered?: number;
  stock?: number;
  revoked?: number;
  faceValueNano?: string;
  lastIssuedAt?: string | null;
};

export type OpenkeysSellersResponse = {
  sellers?: OpenkeysSeller[];
};

export type SellerActionResult = {
  createdBy?: string;
  action?: SellerAction;
  matched?: number;
  changed?: number;
  failed?: number;
  remaining?: number;
};

// Тексты подтверждения. Аннулирование необратимо, поэтому оно требует ввода
// имени издателя — случайный клик по красной кнопке не должен убивать выпуск.
export const SELLER_ACTION_COPY: Record<
  SellerAction,
  { title: string; message: string; confirmLabel: string; danger: boolean }
> = {
  pause: {
    title: "Поставить на паузу ключи",
    message:
      "Все активные ключи этого админа перестанут принимать запросы. Действие обратимо: «снять паузу» вернёт их в строй.",
    confirmLabel: "Поставить на паузу",
    danger: false,
  },
  resume: {
    title: "Снять паузу с ключей",
    message: "Ключи, стоящие на паузе, снова начнут принимать запросы. Аннулированные не воскресают.",
    confirmLabel: "Снять паузу",
    danger: false,
  },
  revoke: {
    title: "Аннулировать ключи",
    message:
      "Необратимо: ключи отключаются в движке навсегда, включая уже выданные покупателям, складские секреты стираются. Введите имя админа, чтобы подтвердить.",
    confirmLabel: "Аннулировать",
    danger: true,
  },
};

// Итог массового действия в одну строку: сколько прошло, что не прошло и
// сколько осталось на следующий клик (сервер режет пачку по потолку).
export function sellerActionToast(result: SellerActionResult): string {
  const changed = result.changed ?? 0;
  const failed = result.failed ?? 0;
  const remaining = result.remaining ?? 0;
  const verb = result.action === "revoke"
    ? "Аннулировано"
    : result.action === "resume"
      ? "Возвращено в строй"
      : "Поставлено на паузу";
  if (changed === 0 && failed === 0 && remaining === 0) return "Подходящих ключей нет — ничего не изменилось.";
  const parts = [`${verb} ключей: ${changed}`];
  if (failed) parts.push(`не удалось: ${failed}`);
  if (remaining) parts.push(`осталось: ${remaining} — нажмите ещё раз`);
  return parts.join(" · ") + ".";
}
