"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { NAV, isNavItemActive } from "@/lib/nav";
import { toggleTheme } from "@/lib/theme";
import { refreshMountedResources } from "@/lib/resources";
import { useRealtimeStatus } from "@/lib/realtime";

// Сайдбар портирован из shell() в admin-panel.js: бренд, группы навигации,
// футер с env, состоянием realtime, точечным обновлением текущего экрана
// и переключателем темы (◐, сохраняется в localStorage).
export function Sidebar() {
  const pathname = usePathname();
  const realtime = useRealtimeStatus();
  return (
    <aside>
      <div className="side-bar-head">
        <div className="brand">
          api<i>Token</i>.sale<small>admin</small>
        </div>
        <a className="skip-link" href="#main-content">К содержанию</a>
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
                <span className="ico" aria-hidden="true">{item.icon}</span>
                {item.label}
              </Link>
            ))}
          </div>
        ))}
      </nav>
      <div className="side-foot">
        <span
          className={`env realtime ${realtime.state}`}
          title={`Realtime-источники: ${realtime.live} из ${realtime.total}`}
          aria-label={`Realtime: ${realtime.live} из ${realtime.total} источников`}
        >
          {realtime.state === "live" ? "live" : realtime.state === "recovering" ? "reconnect" : "connect"}
        </span>
        <button
          type="button"
          className="theme"
          title="Обновить"
          aria-label="Обновить текущую страницу"
          onClick={() => refreshMountedResources()}
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
