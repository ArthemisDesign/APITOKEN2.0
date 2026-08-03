// Единственный источник правды о страницах: сайдбар строится из этого списка.
// Портировано из crates/server/src/admin-panel.js (NAV). Иконки — те же символы.
// Маршруты App Router: Сводка = "/", остальные — "/<tab>". Исключение —
// Подписки = "/subscriptions": путь "/subs" на домене занят data-роутом
// движка (Caddy @admin_data проксирует его на engine раньше фронтенда).
export type NavItem = { href: string; label: string; icon: string };
export type NavGroup = { group: string; items: NavItem[] };

export const NAV: NavGroup[] = [
  { group: "Обзор", items: [{ href: "/", label: "Сводка", icon: "▣" }] },
  {
    group: "Инфраструктура",
    items: [
      { href: "/subscriptions", label: "Подписки", icon: "◍" },
      { href: "/system", label: "Система", icon: "⌘" },
      { href: "/trends", label: "Тренды", icon: "∿" },
    ],
  },
  {
    group: "Клиенты",
    items: [
      { href: "/users", label: "Пользователи", icon: "◉" },
      { href: "/paying-users", label: "Платящие", icon: "◒" },
      { href: "/accounts", label: "Аккаунты", icon: "▤" },
      { href: "/partners", label: "Партнёры", icon: "◆" },
      { href: "/openkeys", label: "OpenKeys", icon: "◈" },
      { href: "/business", label: "B2B", icon: "◇" },
      { href: "/pricing", label: "Pricing", icon: "％" },
    ],
  },
  {
    group: "Деньги",
    items: [
      { href: "/sales/calculator", label: "Калькулятор", icon: "⌁" },
      { href: "/topups", label: "Пополнения", icon: "＄" },
      { href: "/finance", label: "Финансы", icon: "∑" },
    ],
  },
  {
    group: "Управление",
    items: [
      { href: "/admins", label: "Админы", icon: "⚿" },
      { href: "/audit", label: "Аудит", icon: "≡" },
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
