"use client";

import Link from "next/link";
import { Brand } from "@/components/ui";
import { LanguageToggle, useI18n } from "@/components/i18n";
import { ThemeToggle } from "@/components/theme-toggle";

export default function Landing() {
  const { t } = useI18n();
  return (
    <main className="gate">
      <div className="gate-lang gate-tools">
        <LanguageToggle />
        <ThemeToggle />
      </div>
      <div className="gate-card">
        <Brand />
        <p>
          {t(
            "Invite-only partner program for apitoken.sale. You earn a share of what your referrals actually spend on the API — their real, after-discount usage, only the part paid with real money (not top-ups, not free bonus/promo credit).",
            "Партнёрская программа apitoken.sale по приглашениям. Вы зарабатываете долю от того, что ваши рефералы реально тратят на API — их фактического расхода после скидки, только с части, оплаченной реальными деньгами (не с пополнений и не с бесплатного бонуса/промо).",
          )}
        </p>
        <div className="gate-actions">
          <Link href="/login" className="btn btn-primary btn-lg">
            {t("Sign in with Telegram", "Войти через Telegram")}
          </Link>
          <Link href="/register" className="btn btn-ghost btn-lg">
            {t("Apply to join", "Подать заявку")}
          </Link>
        </div>
      </div>
      <p className="gate-foot">
        {t("Main service:", "Основной сервис:")}{" "}
        <a href="https://apitoken.sale" rel="noopener">
          apitoken.sale
        </a>
      </p>
    </main>
  );
}
