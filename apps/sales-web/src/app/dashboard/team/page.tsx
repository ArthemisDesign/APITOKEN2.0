"use client";

// Вкладка Team временно закрыта («Soon»): показываем заблюренный макет без данных
// и без интерактива. Рабочая версия — в истории git, вернётся при запуске
// многоуровневой программы.

import { Card, Table } from "@/components/ui";
import { useI18n } from "@/components/i18n";

const PLACEHOLDER_ROWS = [
  ["@partner_one", "12%", "14", "$1,240.00", "$124.00"],
  ["@partner_two", "10%", "8", "$610.50", "$61.05"],
  ["@partner_three", "10%", "3", "$95.20", "$9.52"],
];

export default function TeamPage() {
  const { t } = useI18n();
  return (
    <>
      <h1 className="page-title">{t("Team", "Команда")}</h1>
      <p className="page-sub">
        {t(
          "Recruit sub-partners and earn an override on their results.",
          "Приглашайте суб-партнёров и получайте надбавку с их результатов.",
        )}
      </p>

      <div className="soon-wrap">
        <div className="soon-overlay" role="status">
          <span className="soon-label">{t("Soon", "Скоро")}</span>
          <p>{t("Sub-partner teams are launching soon.", "Команды суб-партнёров скоро запустятся.")}</p>
        </div>

        <div className="soon-blur" aria-hidden inert>
          <div className="stack">
            <Card
              title={t("Invite a sub-partner", "Пригласить суб-партнёра")}
              sub={t(
                "Enter their Telegram username and send them the link.",
                "Введите его имя пользователя в Telegram и отправьте ему ссылку.",
              )}
            >
              <div className="reflink-row">
                <input className="input" readOnly value="@telegram_username" />
                <button className="btn btn-primary" type="button" tabIndex={-1}>
                  {t("Create invite", "Создать приглашение")}
                </button>
              </div>
            </Card>
            <Card title={t("Your sub-partners", "Ваши суб-партнёры")}>
              <Table
                head={
                  <>
                    <th>{t("Partner", "Партнёр")}</th>
                    <th>{t("Commission", "Комиссия")}</th>
                    <th className="num">{t("Referred users", "Приглашённые")}</th>
                    <th className="num">{t("They earned", "Они заработали")}</th>
                    <th className="num">{t("Your override", "Ваша надбавка")}</th>
                  </>
                }
              >
                {PLACEHOLDER_ROWS.map((row) => (
                  <tr key={row[0]}>
                    {row.map((cell, index) => (
                      <td key={index} className={index >= 2 ? "num" : undefined}>
                        {cell}
                      </td>
                    ))}
                  </tr>
                ))}
              </Table>
            </Card>
          </div>
        </div>
      </div>
    </>
  );
}
