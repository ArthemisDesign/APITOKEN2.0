"use client";

// Вкладка Team временно закрыта («Soon»): показываем заблюренный макет без данных
// и без интерактива. Рабочая версия — в истории git, вернётся при запуске
// многоуровневой программы.

import { Card, Table } from "@/components/ui";

const PLACEHOLDER_ROWS = [
  ["@partner_one", "12%", "14", "$1,240.00", "$124.00"],
  ["@partner_two", "10%", "8", "$610.50", "$61.05"],
  ["@partner_three", "10%", "3", "$95.20", "$9.52"],
];

export default function TeamPage() {
  return (
    <>
      <h1 className="page-title">Team</h1>
      <p className="page-sub">Recruit sub-partners and earn an override on their results.</p>

      <div className="soon-wrap">
        <div className="soon-overlay" role="status">
          <span className="soon-label">Soon</span>
          <p>Sub-partner teams are launching soon.</p>
        </div>

        <div className="soon-blur" aria-hidden inert>
          <div className="stack">
            <Card title="Invite a sub-partner" sub="Enter their Telegram username and send them the link.">
              <div className="reflink-row">
                <input className="input" readOnly value="@telegram_username" />
                <button className="btn btn-primary" type="button" tabIndex={-1}>
                  Create invite
                </button>
              </div>
            </Card>
            <Card title="Your sub-partners">
              <Table
                head={
                  <>
                    <th>Partner</th>
                    <th>Commission</th>
                    <th className="num">Referred users</th>
                    <th className="num">They earned</th>
                    <th className="num">Your override</th>
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
