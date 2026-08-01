"use client";

// Документация партнёрской программы прямо в кабинете. Двуязычная (RU/EN через useI18n),
// персонализированная (ставка и реф-код партнёра). Полная версия для команды — docs/sales/PARTNER_PROGRAM.md.

import { formatBps } from "@/lib/api";
import { usePartner } from "@/components/partner-context";
import { CommissionFormula } from "@/components/commission-formula";
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
                "You bring users to apitoken.sale with your referral link and earn a percentage of what they actually spend on API usage.",
                "Вы приводите пользователей на apitoken.sale по своей реферальной ссылке и зарабатываете процент от того, что они реально тратят на использование API.",
              )}
            </li>
            <li>
              {t("Your current rate: ", "Ваша текущая ставка: ")}
              <strong>{rate}</strong>
              {t(" of your referrals' real spend on the API.", " от реальных трат ваших рефералов на API.")}
            </li>
            <li>
              {t(
                "Free credits never count: usage paid from a welcome bonus, promo codes or any gifted balance earns you nothing. Free money is spent first — only spend covered by their real money counts.",
                "Бесплатные средства не считаются никогда: использование, оплаченное из приветственного бонуса, промокодов или любого подаренного баланса, комиссию не приносит. Бесплатное тратится первым — засчитывается только трата, покрытая их реальными деньгами.",
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
          <div style={{ marginBottom: 16 }}>
            <CommissionFormula commissionBps={partner.commissionBps} />
          </div>
          <ul className="how-list">
            <li>
              {t(
                "The base is what a referral actually pays for API usage — the amount charged after their own discount — and only the part covered by their real money (free bonus/promo balance is spent first and never counts).",
                "База — это то, что реферал реально платит за использование API: сумма, списанная с учётом его скидки, и только часть, покрытая его реальными деньгами (бесплатный бонус/промо тратится первым и не считается).",
              )}
            </li>
            <li>
              {t("Commission = ", "Комиссия = ")}
              <strong>{rate}</strong>
              {t(" × their real-money spend on the API.", " × их реальные траты на API.")}
            </li>
            <li>
              {t(
                "Example: your referral spends $100 of real money on the API → you earn ",
                "Пример: ваш реферал тратит $100 реальных денег на API → вы получаете ",
              )}
              <strong>{formatBpsAmount(partner.commissionBps, 100)}</strong>
              {t(" — your ", " — ваши ")}
              <strong>{rate}</strong>
              {t(" of their spend.", " от его трат.")}
            </li>
            <li>
              {t(
                "Commission accrues only while your account is active. A suspended or not-yet-approved account does not earn, and existing referrals' spend does not accrue during that time.",
                "Комиссия начисляется только пока ваш аккаунт активен. Приостановленный или ещё не одобренный аккаунт не зарабатывает, и траты уже привлечённых рефералов в это время не начисляются.",
              )}
            </li>
          </ul>
        </Card>

        <Card title={t("4. Sub-partners", "4. Суб-партнёры")}>
          <ul className="how-list">
            <li>
              {t(
                "You can invite other sales partners and earn an override — your separate sub-partner rate — on what partners below you earn, up to 10 levels deep. Each level earns your override rate applied to the level directly below it.",
                "Вы можете приглашать других сейлзов и получать override — вашу отдельную ставку для суб-партнёров — с того, что зарабатывают партнёры под вами, на глубину до 10 уровней. Каждый уровень получает вашу override-ставку от уровня прямо под ним.",
              )}
            </li>
            <li>
              {t(
                "The override rate is set per partner and is separate from your direct commission; you cannot grant a sub-partner a higher rate than your own. (The Team section opens soon.)",
                "Override-ставка задаётся индивидуально и отличается от вашей прямой комиссии; нельзя дать суб-партнёру ставку выше своей собственной. (Раздел «Команда» откроется скоро.)",
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
                "Your referrals' identities are hidden — you only see a masked id, their spend and your earnings per user.",
                "Личности ваших рефералов скрыты — вы видите только маскированный идентификатор, их траты и ваш заработок с каждого.",
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
