"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { NAV, isNavItemActive } from "@/lib/nav";
import { toggleTheme } from "@/lib/theme";
import { refreshMountedResources } from "@/lib/resources";
import { useRealtimeStatus } from "@/lib/realtime";
import { LanguageToggle, useI18n } from "@/lib/i18n";

// Сайдбар портирован из shell() в admin-panel.js: бренд, группы навигации,
// футер с env, состоянием realtime, точечным обновлением текущего экрана
// и переключателем темы (◐, сохраняется в localStorage).
export function Sidebar() {
  const pathname = usePathname();
  const realtime = useRealtimeStatus();
  const { t } = useI18n();
  return (
    <aside>
      <div className="side-bar-head">
        <div className="brand">
          api<i>Token</i>.sale<small>admin</small>
        </div>
        <a className="skip-link" href="#main-content">{t("Skip to content", "К содержанию")}</a>
      </div>
      <nav aria-label={t("Admin sections", "Разделы админ-панели")}>
        {NAV.map((group) => (
          <div key={group.group}>
            <div className="nav-group">{t(group.groupEn ?? group.group, group.group)}</div>
            {group.items.map((item) => (
              <Link
                key={item.href}
                className={"nav-item" + (isNavItemActive(pathname, item.href) ? " on" : "")}
                href={item.href}
                prefetch={false}
              >
                <span className="ico" aria-hidden="true">{item.icon}</span>
                {t(item.labelEn ?? item.label, item.label)}
              </Link>
            ))}
          </div>
        ))}
      </nav>
      <div className="side-foot">
        <span
          className={`env realtime ${realtime.state}`}
          title={t(`Realtime sources: ${realtime.live} of ${realtime.total}`, `Realtime-источники: ${realtime.live} из ${realtime.total}`)}
          aria-label={t(`Realtime: ${realtime.live} of ${realtime.total} sources`, `Realtime: ${realtime.live} из ${realtime.total} источников`)}
        >
          {realtime.state === "live" ? "live" : realtime.state === "recovering" ? "reconnect" : "connect"}
        </span>
        <LanguageToggle />
        <button
          type="button"
          className="theme"
          title={t("Refresh", "Обновить")}
          aria-label={t("Refresh current page", "Обновить текущую страницу")}
          onClick={() => refreshMountedResources()}
        >
          ↻
        </button>
        <button
          type="button"
          className="theme"
          title={t("Change theme", "Сменить тему")}
          aria-label={t("Change theme", "Сменить тему")}
          onClick={() => toggleTheme()}
        >
          ◐
        </button>
      </div>
    </aside>
  );
}
