"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useState } from "react";
import { NAV, isNavItemActive } from "@/lib/nav";
import { toggleTheme } from "@/lib/theme";
import { refreshMountedResources } from "@/lib/resources";
import { useRealtimeStatus } from "@/lib/realtime";
import { LanguageToggle, useI18n } from "@/lib/i18n";

const MOBILE_NAV_QUERY = "(max-width: 1023px)";

// Сайдбар портирован из shell() в admin-panel.js: бренд, группы навигации,
// футер с env, состоянием realtime, точечным обновлением текущего экрана
// и переключателем темы (◐, сохраняется в localStorage).
// На viewport ≤1023px (включая iPhone 14 Pro Max portrait и landscape) это
// липкая шапка и выезжающий список разделов. Горизонтальная лента из 18 пунктов
// оставляла на экране только три ссылки и прятала шапку под Error Center.
export function Sidebar() {
  const pathname = usePathname();
  const realtime = useRealtimeStatus();
  const { t } = useI18n();
  const [open, setOpen] = useState(false);

  useEffect(() => {
    setOpen(false);
  }, [pathname]);

  useEffect(() => {
    const media = window.matchMedia(MOBILE_NAV_QUERY);
    const onChange = () => {
      if (!media.matches) setOpen(false);
    };
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, []);

  useEffect(() => {
    if (!open) return;
    const main = document.getElementById("main-content");
    const previousBody = document.body.style.overflow;
    const previousHtml = document.documentElement.style.overflow;
    document.body.style.overflow = "hidden";
    document.documentElement.style.overflow = "hidden";
    if (main) main.inert = true;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        setOpen(false);
      }
    };
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.body.style.overflow = previousBody;
      document.documentElement.style.overflow = previousHtml;
      if (main) main.inert = false;
    };
  }, [open]);

  return (
    <aside className={open ? "nav-open" : undefined}>
      <div className="side-bar-head">
        <div className="brand">
          api<i>Token</i>.sale<small>admin</small>
        </div>
        <a className="skip-link" href="#main-content">{t("Skip to content", "К содержанию")}</a>
      </div>
      <nav id="admin-nav" aria-label={t("Admin sections", "Разделы админ-панели")}>
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
      <button
        type="button"
        className={"burger" + (open ? " open" : "")}
        aria-expanded={open}
        aria-controls="admin-nav"
        aria-label={open ? t("Close menu", "Закрыть меню") : t("Open menu", "Открыть меню")}
        onClick={() => setOpen((value) => !value)}
      >
        <span className="burger-icon" aria-hidden="true" />
      </button>
    </aside>
  );
}
