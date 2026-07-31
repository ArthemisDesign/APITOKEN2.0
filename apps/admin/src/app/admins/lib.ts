// Чистая логика страницы «Админы» — вынесена из page.tsx для юнит-тестов.
// Портирована 1:1 из adminAction() в crates/server/src/admin-panel.js (строки 376-393).

// Разбор поля «Домены» из диалога: через запятую, trim, дедупликация.
// null — пустой список или хотя бы один домен вне разрешённых (как в легаси:
// «Укажите один или несколько доменов ровно как в списке.»).
export function parseDomainsInput(raw: string, allowed: string[]): string[] | null {
  const selected = [
    ...new Set(
      raw
        .split(",")
        .map((item) => item.trim())
        .filter(Boolean),
    ),
  ];
  if (!selected.length || selected.some((item) => !allowed.includes(item))) return null;
  return selected;
}

// Страховка «нельзя отключить последнего активного администратора» (сервер тоже
// проверяет): true, если аккаунт — единственный активный в переданном списке.
export function isLastActiveAdmin(accounts: { id?: string; status?: string }[], id: string): boolean {
  const active = accounts.filter((account) => account.status === "active");
  return active.length === 1 && active[0].id === id;
}
