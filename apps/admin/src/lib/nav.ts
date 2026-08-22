// Единственный источник правды о страницах: сайдбар строится из этого списка.
// Портировано из crates/server/src/admin-panel.js (NAV). Иконки — те же символы.
// Маршруты App Router: Сводка = "/", остальные — "/<tab>". Исключение —
// Подписки = "/subscriptions": путь "/subs" на домене занят data-роутом
// движка (Caddy @admin_data проксирует его на engine раньше фронтенда).
export type NavItem = { href: string; label: string; labelEn?: string; icon: string };
export type NavGroup = { group: string; groupEn?: string; items: NavItem[] };

export const NAV: NavGroup[] = [
  { group: "Обзор", groupEn: "Overview", items: [{ href: "/", label: "Сводка", labelEn: "Dashboard", icon: "▣" }] },
  {
    group: "Инфраструктура", groupEn: "Infrastructure",
    items: [
      { href: "/subscriptions", label: "Подписки", labelEn: "Subscriptions", icon: "◍" },
      { href: "/proxies", label: "Прокси", labelEn: "Proxies", icon: "◎" },
      { href: "/system", label: "Система", labelEn: "System", icon: "⌘" },
      { href: "/trends", label: "Тренды", labelEn: "Trends", icon: "∿" },
    ],
  },
  {
    group: "Клиенты", groupEn: "Customers",
    items: [
      { href: "/users", label: "Пользователи", labelEn: "Users", icon: "◉" },
      { href: "/paying-users", label: "Платящие", labelEn: "Paying users", icon: "◒" },
      { href: "/accounts", label: "Аккаунты", labelEn: "Accounts", icon: "▤" },
      { href: "/partners", label: "Партнёры", labelEn: "Partners", icon: "◆" },
      { href: "/openkeys", label: "OpenKeys", labelEn: "OpenKeys", icon: "◈" },
      { href: "/business", label: "B2B", labelEn: "B2B", icon: "◇" },
    ],
  },
  {
    group: "Деньги", groupEn: "Money",
    items: [
      { href: "/sales/calculator", label: "Калькулятор", labelEn: "Calculator", icon: "⌁" },
      { href: "/topups", label: "Пополнения", labelEn: "Top-ups", icon: "＄" },
      { href: "/engine-spend", label: "Расход движка", labelEn: "Engine spend", icon: "⟠" },
      { href: "/request-analytics", label: "Request Analytics", icon: "⌬" },
      { href: "/finance", label: "Финансы", labelEn: "Finance", icon: "∑" },
    ],
  },
  {
    group: "Управление", groupEn: "Management",
    items: [
      { href: "/admins", label: "Админы", labelEn: "Admins", icon: "⚿" },
      { href: "/audit", label: "Аудит", labelEn: "Audit", icon: "≡" },
    ],
  },
];

export function isNavItemActive(pathname: string, href: string): boolean {
  return href === "/" ? pathname === "/" : pathname.startsWith(href);
}

export function navLabelForPath(pathname: string): string {
  for (const group of NAV) {
    for (const item of group.items) {
      if (isNavItemActive(pathname, item.href)) return item.label;
    }
  }
  return "Сводка";
}
