"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { Suspense } from "react";
import { LoadingGrid } from "@/components/ui";
import { useI18n } from "@/lib/i18n";

const ITEMS = [
  { href: "/partners", en: "Overview", ru: "Обзор", exact: true },
  { href: "/partners/requests", en: "Requests", ru: "Заявки" },
  { href: "/partners/onboarding", en: "Enable Partner", ru: "Сделать партнёром" },
  { href: "/partners/directory", en: "Directory", ru: "Партнёры" },
  { href: "/partners/payouts", en: "Payouts", ru: "Выплаты" },
] as const;

export default function PartnersLayout({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  const { t } = useI18n();
  return <div className="partners-control-room">
    <nav className="partners-subnav" aria-label={t("Partner management", "Управление партнёрами")}>
      {ITEMS.map((item) => {
        const active = "exact" in item && item.exact ? pathname === item.href : pathname.startsWith(item.href);
        return <Link key={item.href} href={item.href} aria-current={active ? "page" : undefined} className={active ? "on" : ""}>{t(item.en, item.ru)}</Link>;
      })}
    </nav>
    <Suspense fallback={<LoadingGrid label={t("Loading Partner Management", "Загрузка управления партнёрами")} />}>
      {children}
    </Suspense>
  </div>;
}
