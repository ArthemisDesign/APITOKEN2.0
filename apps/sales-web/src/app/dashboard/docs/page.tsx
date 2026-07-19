"use client";

// Документация партнёрской программы прямо в кабинете. Двуязычная (RU/EN через useI18n),
// персонализированная (ставка и реф-код партнёра). Полная версия для команды — docs/PARTNER_PROGRAM.md.

import { formatBps } from "@/lib/api";
import { usePartner } from "@/components/partner-context";
import { useI18n } from "@/components/i18n";
import { Card } from "@/components/ui";

export default function DocsPage() {
  const { t } = useI18n();
  const partner = usePartner();
  const rate = formatBps(partner.commissionBps);

  return (
    <>
      <h1 className="page-title">{t("Documentation", "Документация")}</h1>
      <p className="page-sub">
        {t(
          "How the partner program works — from your referral link to money on your wallet.",
          "Как устроена партнёрская программа — от вашей реферальной ссылки до денег на кошельке.",
        )}
      </p>

      <div className="stack">
        <Card title={t("1. What you earn", "1. За что вы зарабатываете")}>
          <ul className="how-list">
            <li>
              {t(
                "You bring users to apitoken.sale with your referral link and earn a percentage of the real money they deposit (top up with their own money).",
                "Вы приводите пользователей на apitoken.sale по своей реферальной ссылке и зарабатываете процент от реальных денег, которые они вносят (пополняют своими деньгами).",
              )}
            </li>
            <li>
              {t("Your current rate: ", "Ваша текущая ставка: ")}
              <strong>{rate}</strong>
              {t(" of your referrals' real-money deposits.", " от реальных пополнений ваших рефералов.")}
            </li>
            <li>
              {t(
                "Free credits never count: welcome bonuses, promo codes and any other gifted balance earn you nothing — commission is paid only on money paid in with real money.",
                "Бесплатные средства не считаются никогда: приветственные бонусы, промокоды и любой другой подаренный баланс комиссию не приносят — начисляется только с денег, внесённых реальными деньгами.",
              )}
            </li>
          </ul>
        </Card>

        <Card title={t("2. Your referral link", "2. Ваша реферальная ссылка")}>
          <ul className="how-list">
            <li>
              {t("Your code is ", "Ваш код — ")}
              <span className="mono">{partner.referralCode}</span>
              {t("; share the link from the Overview page.", "; ссылку можно скопировать на странице «Обзор».")}
            </li>
            <li>
              {t(
                "When someone opens your link, the code is remembered in their browser for 30 days and permanently attributes them to you once they register on apitoken.sale.",
                "Когда человек переходит по вашей ссылке, код запоминается в его браузере на 30 дней и навсегда закрепляет его за вами, как только он регистрируется на apitoken.sale.",
              )}
            </li>
            <li>
              {t(
                "Attribution only affects who gets the commission — it never changes the price or discount for the user.",
                "Атрибуция влияет только на то, кому идёт комиссия — на цену или скидку пользователя она не влияет.",
              )}
            </li>
          </ul>
        </Card>

        <Card title={t("3. How commission is calculated", "3. Как считается комиссия")}>
          <ul className="how-list">
            <li>
              {t(
                "The base is the real money a referral deposits — what they actually pay in with their own money at the discounted price. Free credits (bonus, promo) are not deposits and never count.",
                "База — это реальные деньги, которые реферал вносит: то, что он реально оплатил своими деньгами по своей цене со скидкой. Бесплатные средства (бонус, промо) депозитами не являются и не считаются никогда.",
              )}
            </li>
            <li>
              {t("Commission = ", "Комиссия = ")}
              <strong>{rate}</strong>
              {t(" × the real money the referral deposited.", " × реальные деньги, внесённые рефералом.")}
            </li>
            <li>
              {t(
                "Example: your referral deposits $100 of their own money → you earn ",
                "Пример: ваш реферал вносит $100 своих денег → вы получаете ",
              )}
              <strong>{formatBpsAmount(partner.commissionBps, 100)}</strong>
              {t(" — your ", " — ваши ")}
              <strong>{rate}</strong>
              {t(" of their deposit.", " от его пополнения.")}
            </li>
          </ul>
        </Card>

        <Card title={t("4. Sub-partners", "4. Суб-партнёры")}>
          <ul className="how-list">
            <li>
              {t(
                "You can invite other sales partners and earn an override — a percentage of what they earn — up the chain.",
                "Вы можете приглашать других сейлзов и получать override — процент от того, что зарабатывают они — вверх по цепочке.",
              )}
            </li>
            <li>
              {t(
                "You cannot grant a sub-partner a higher rate than your own. (The Team section opens soon.)",
                "Нельзя дать суб-партнёру ставку выше своей собственной. (Раздел «Команда» откроется скоро.)",
              )}
            </li>
          </ul>
        </Card>

        <Card title={t("5. Wallet & currency", "5. Кошелёк и валюта")}>
          <ul className="how-list">
            <li>
              {t(
                "Payouts are made only in USDT (BEP-20) on BNB Smart Chain (BSC). Bind your BSC wallet on the Payouts page.",
                "Выплаты — только в USDT (BEP-20) в сети BNB Smart Chain (BSC). Привяжите свой BSC-кошелёк на странице «Выплаты».",
              )}
            </li>
            <li>
              {t(
                "No wallet yet? Nothing is lost — your balance keeps accruing and is paid as soon as a wallet is bound.",
                "Кошелёк ещё не привязан? Ничего не теряется — баланс продолжает копиться и будет выплачен, как только появится кошелёк.",
              )}
            </li>
          </ul>
        </Card>

        <Card title={t("6. Payout schedule", "6. График выплат")}>
          <ul className="how-list">
            <li>
              {t(
                "Earnings are counted in two periods each month — the 1st–15th and the 16th–last day of the month. Two payouts a month.",
                "Заработок считается по двум периодам в месяц — с 1-го по 15-е и с 16-го по последний день месяца. Две выплаты в месяц.",
              )}
            </li>
            <li>
              {t(
                "After a period ends there is a 7-day lock, then any earnings are sent to your wallet within the next 3 days.",
                "После конца периода идёт лок 7 дней, затем в течение 3 дней любой заработок уходит на ваш кошелёк.",
              )}
            </li>
            <li>
              {t(
                "Any amount above zero is paid — there is no minimum. If a payout can't be sent (no wallet), the balance rolls into the next one, so it may cover two periods at once.",
                "Выплачивается любая сумма больше нуля — минимума нет. Если выплату нельзя отправить (нет кошелька), баланс переносится на следующую — тогда она может прийти сразу за два периода.",
              )}
            </li>
            <li>
              {t(
                "Track it live on the Payouts page: current period, lock, next payout date and period history.",
                "Следить за этим можно на странице «Выплаты»: текущий период, лок, дата следующей выплаты и история по периодам.",
              )}
            </li>
          </ul>
        </Card>

        <Card title={t("7. Privacy", "7. Приватность")}>
          <ul className="how-list">
            <li>
              {t(
                "Your referrals' identities are hidden — you only see a masked id, their deposits and your earnings per user.",
                "Личности ваших рефералов скрыты — вы видите только маскированный идентификатор, их пополнения и ваш заработок с каждого.",
              )}
            </li>
          </ul>
        </Card>
      </div>
    </>
  );
}

// $ from bps × dollars, integer-safe for the docs example.
function formatBpsAmount(bps: number, dollars: number): string {
  const cents = Math.floor((bps * dollars * 100) / 10000);
  return `$${(cents / 100).toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;
}
