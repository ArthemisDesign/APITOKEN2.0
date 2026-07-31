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
