"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { NAV, isNavItemActive } from "@/lib/nav";
import { toggleTheme } from "@/lib/theme";
import { revalidateAll } from "@/lib/usePoll";

// Сайдбар портирован из shell() в admin-panel.js: бренд, группы навигации,
// футер с env, кнопкой ручного обновления (↻ — ревалидация всех poller'ов)
// и переключателем темы (◐, сохраняется в localStorage).
export function Sidebar() {
  const pathname = usePathname();
  return (
    <aside>
      <div className="brand">
        api<i>Token</i>.sale<small>admin</small>
      </div>
      <nav aria-label="Разделы админ-панели">
        {NAV.map((group) => (
          <div key={group.group}>
            <div className="nav-group">{group.group}</div>
            {group.items.map((item) => (
              <Link
                key={item.href}
                className={"nav-item" + (isNavItemActive(pathname, item.href) ? " on" : "")}
                href={item.href}
              >
                <span className="ico">{item.icon}</span>
                {item.label}
              </Link>
            ))}
          </div>
        ))}
      </nav>
      <div className="side-foot">
        <span className="env">production</span>
        <button
          type="button"
          className="theme"
          title="Обновить"
          aria-label="Обновить текущую страницу"
          onClick={() => revalidateAll()}
        >
          ↻
        </button>
        <button
          type="button"
          className="theme"
          title="Сменить тему"
          aria-label="Сменить тему"
          onClick={() => toggleTheme()}
        >
          ◐
        </button>
      </div>
    </aside>
  );
}
