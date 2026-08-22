// Человекочитаемые имена источников данных — порт sourceName() из admin-panel.js
// (строки 97-107). Ключ — путь API без query-строки; неизвестный путь возвращается
// как есть. Используется центром ошибок (ErrorCenter) и доступен страницам.
const SOURCE_NAMES: Record<string, string> = {
  "/admin/dashboard": "Коммерческая сводка",
  "/overview": "Движок",
  "/capacity": "Ёмкость флота",
  "/proxy-admin/inventory": "Реестр прокси",
  "/subs": "Claude-подписки",
  "/codex-subs": "GPT-подписки",
  "/gemini-subs": "Gemini-подписки",
  "/kimi-subs": "KIMI-подписки",
  "/glm-subs": "GLM-подписки",
  "/tripo3d-subs": "Tripo3D-подписки",
  "/suno-subs": "Suno-подписки",
  "/fleet-history": "История флота",
  "/admin/referral/partners": "Партнёрские аккаунты Commerce",
  "/admin/referral/requests": "Заявки партнёров",
  "/admin/referral/payouts": "Заявки на выплату партнёрам",
  "/partner-admin/payouts/engine": "Окно выплат",
  "/partner-admin/payouts/batches": "On-chain батчи",
  "/admin/users": "Пользователи",
  "/admin/topups": "Пополнения",
  "/admin/audit": "Аудит",
  "/admin/audit/actions": "Действия аудита",
  "/admin/business-invites": "B2B-инвайты",
  "/admin/finance/overview": "Финансовая сводка",
  "/admin/finance/revenue": "Выручка по дням",
  "/admin/finance/funnel": "Воронка чекаутов",
  "/admin/finance/top-customers": "Топ клиентов",
  "/admin/finance/paying-users": "Платящие клиенты",
  "/admin/finance/engine-spend": "Расход движка",
  "/admin/refunds": "Возвраты",
  "/admin/finance/cohorts": "Когорты",
  "/admin/finance/churn-signals": "Сигналы оттока",
  "/openkeys-admin/keys": "Ключи OpenKeys",
  "/admin/admin-accounts": "Администраторы",
  "/admin/admin-accounts/domains": "Домены администраторов",
  "/spend-stats": "Статистика расхода",
  "/admin/pipeline-health": "Здоровье пайплайнов",
  "/settlement-health": "Settlement движка",
};

// sourceName("/admin/users?limit=50") → "Пользователи"; неизвестный → сам путь без query.
export function sourceName(path: string): string {
  const key = path.split("?")[0];
  return SOURCE_NAMES[key] ?? key;
}
