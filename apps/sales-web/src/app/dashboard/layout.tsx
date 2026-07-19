"use client";

import { useEffect, useState, type ReactNode } from "react";
import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { api, ApiError, type Partner } from "@/lib/api";
import { Brand, Loading } from "@/components/ui";
import { PartnerContext } from "@/components/partner-context";
import { LanguageToggle, useI18n } from "@/components/i18n";

const NAV = [
  { href: "/dashboard", en: "Overview", ru: "Обзор", icon: "◧" },
  { href: "/dashboard/referrals", en: "Referrals", ru: "Рефералы", icon: "⇢" },
  { href: "/dashboard/team", en: "Team", ru: "Команда", icon: "⁂", soon: true },
  { href: "/dashboard/payouts", en: "Payouts", ru: "Выплаты", icon: "◈" },
  { href: "/dashboard/docs", en: "Docs", ru: "Документация", icon: "▤" },
  { href: "/dashboard/settings", en: "Settings", ru: "Настройки", icon: "⚙" },
];

export default function DashboardLayout({ children }: { children: ReactNode }) {
  const { t } = useI18n();
  const router = useRouter();
  const pathname = usePathname();
  const [partner, setPartner] = useState<Partner | null>(null);
  const [checking, setChecking] = useState(true);
  const [menuOpen, setMenuOpen] = useState(false);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const res = await api<{ partner: Partner }>("/v1/auth/me");
        if (!cancelled) {
          setPartner(res.partner);
          setChecking(false);
        }
      } catch (err) {
        if (cancelled) return;
        if (err instanceof ApiError && err.status === 401) {
          router.replace("/login");
        } else {
          setChecking(false);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [router]);

  // Close the mobile menu on navigation.
  useEffect(() => {
    setMenuOpen(false);
  }, [pathname]);

  async function logout() {
    try {
      await api("/v1/auth/logout", { method: "POST" });
    } catch {
      // ignore — redirect regardless
    }
    router.replace("/login");
  }

  if (checking || !partner) {
    return (
      <div className="auth-shell" style={{ justifyContent: "center" }}>
        {checking ? (
          <Loading label={t("Loading your cabinet…", "Загружаем ваш кабинет…")} />
        ) : (
          <div className="auth-card">
            <h1>{t("Can't reach the partner API", "Не удаётся связаться с API партнёров")}</h1>
            <p className="auth-sub">
              {t("Check your connection and reload the page.", "Проверьте соединение и перезагрузите страницу.")}
            </p>
            <button className="btn btn-primary" onClick={() => window.location.reload()}>
              {t("Reload", "Перезагрузить")}
            </button>
          </div>
        )}
      </div>
    );
  }

  return (
    <PartnerContext.Provider value={partner}>
      <div className="cab">
        <aside className={`cab-sidebar${menuOpen ? " open" : ""}`}>
          <Link href="/dashboard" className="brand">
            <Brand />
          </Link>
          <nav className="cab-nav">
            {NAV.map((item) => {
              const active =
                item.href === "/dashboard"
                  ? pathname === "/dashboard"
                  : pathname.startsWith(item.href);
              return (
                <Link
                  key={item.href}
                  href={item.href}
                  className={`cab-link${active ? " active" : ""}`}
                >
                  <span aria-hidden style={{ width: 18, textAlign: "center" }}>
                    {item.icon}
                  </span>
                  {t(item.en, item.ru)}
                  {"soon" in item && item.soon ? (
                    <span className="soon-pill">{t("Soon", "Скоро")}</span>
                  ) : null}
                </Link>
              );
            })}
          </nav>
          <div className="cab-side-foot">
            {t("Partner code:", "Код партнёра:")} <span className="mono">{partner.referralCode}</span>
          </div>
        </aside>
        {menuOpen ? (
          <div className="cab-overlay" onClick={() => setMenuOpen(false)} aria-hidden />
        ) : null}
        <div className="cab-main">
          <header className="cab-topbar">
            <button
              className="cab-burger"
              onClick={() => setMenuOpen((v) => !v)}
              aria-label={t("Toggle menu", "Открыть меню")}
            >
              ☰
            </button>
            <div className="cab-topbar-user">
              <span className="email">
                {partner.telegramUsername ? `@${partner.telegramUsername}` : partner.displayName ?? partner.email}
              </span>
              <LanguageToggle />
              <button className="btn btn-ghost btn-sm" onClick={logout}>
                {t("Log out", "Выйти")}
              </button>
            </div>
          </header>
          <main className="cab-content">{children}</main>
        </div>
      </div>
    </PartnerContext.Provider>
  );
}
