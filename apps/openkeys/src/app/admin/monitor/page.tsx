"use client";

import Link from "next/link";
import { useCallback, useEffect, useState } from "react";
import { AppShell } from "@/components/app-shell";
import { formatNanoUsd } from "@/lib/format";

interface MonitorRow {
  id: string;
  status: "stock" | "delivered";
  keyMasked: string;
  label: string | null;
  faceValue: string;
  viewUrl: string;
  createdAt: string;
  deliveredAt: string | null;
  remaining: string | null;
  spent: string | null;
  spentNano: string | null;
  enabled: boolean | null;
}

export default function MonitorPage() {
  const [authorized, setAuthorized] = useState(true);
  const [loading, setLoading] = useState(true);
  const [rows, setRows] = useState<MonitorRow[]>([]);
  const [onlyDelivered, setOnlyDelivered] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const response = await fetch("/api/admin/monitor", { cache: "no-store" });
      if (response.status === 401) {
        setAuthorized(false);
        return;
      }
      const payload = (await response.json()) as { rows: MonitorRow[] };
      setAuthorized(true);
      setRows(payload.rows);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  async function toggle(row: MonitorRow) {
    const next = !(row.enabled ?? true);
    if (!next && !window.confirm("Отключить ключ? Запросы по нему перестанут проходить, ключ останется в системе.")) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const response = await fetch("/api/admin/monitor", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ id: row.id, enabled: next }),
      });
      if (!response.ok) {
        setError("Не удалось изменить статус ключа");
        return;
      }
      await refresh();
    } finally {
      setBusy(false);
    }
  }

  if (!authorized) {
    return (
      <AppShell section="monitor" title="Наблюдение">
        <div className="app-body">
          <div className="app-body-in">
            <div className="empty-box">
              Сессия истекла — <Link href="/admin">войдите заново</Link>
            </div>
          </div>
        </div>
      </AppShell>
    );
  }

  const visible = onlyDelivered ? rows.filter((row) => row.status === "delivered") : rows;
  const totalSpentNano = visible.reduce((sum, row) => sum + BigInt(row.spentNano ?? "0"), 0n);
  const totalSpent = formatNanoUsd(totalSpentNano, 2, 2);
  const active = visible.filter((row) => row.enabled !== false).length;

  return (
    <AppShell
      section="monitor"
      title="Наблюдение за ключами"
      actions={
        <>
          <button className="btn btn-ghost btn-sm" type="button" disabled={loading} onClick={() => void refresh()}>
            {loading ? "Обновляем…" : "Обновить"}
          </button>
        </>
      }
    >
      <div className="app-body">
        <div className="app-body-in">
          {error ? <div className="banner banner-error">{error}</div> : null}

          <div className="ov-stats bill4">
            <div className="ovstat">
              <span className="dlabel">Ключей</span>
              <b className="num">{visible.length}</b>
              <span className="dtrend">{onlyDelivered ? "только выданные" : "склад и выданные"}</span>
            </div>
            <div className="ovstat">
              <span className="dlabel">Активны</span>
              <b className="num">{active}</b>
              <span className="dtrend">принимают запросы</span>
            </div>
            <div className="ovstat">
              <span className="dlabel">Потрачено всего</span>
              <b className="num accent">{totalSpent}</b>
              <span className="dtrend">по официальным прайсам использованных моделей</span>
            </div>
            <div className="ovstat">
              <span className="dlabel">Фильтр</span>
              <button
                className="btn btn-ghost btn-sm"
                type="button"
                onClick={() => setOnlyDelivered((value) => !value)}
              >
                {onlyDelivered ? "Показать все" : "Только выданные"}
              </button>
            </div>
          </div>

          <section className="dsec">
            <div className="dsec-head analytics-heading">
              <div>
                <h2>Ключи</h2>
                <p>Остаток и расход берутся у движка в момент открытия страницы.</p>
              </div>
            </div>

            {loading && rows.length === 0 ? (
              <div className="empty-box">Опрашиваем движок…</div>
            ) : visible.length === 0 ? (
              <div className="empty-box">Ключей пока нет</div>
            ) : (
              <div className="table-scroll" role="region" tabIndex={0} aria-label="Наблюдение за ключами">
                <table className="mtable">
                  <thead>
                    <tr>
                      <th>Ключ</th>
                      <th>Метка</th>
                      <th>Состояние</th>
                      <th className="tnum">Номинал</th>
                      <th className="tnum">Остаток</th>
                      <th className="tnum">Потрачено</th>
                      <th>Профиль</th>
                      <th />
                    </tr>
                  </thead>
                  <tbody>
                    {visible.map((row) => (
                      <tr key={row.id}>
                        <td>
                          <code className="key-mask">{row.keyMasked}</code>
                        </td>
                        <td>{row.label ?? "—"}</td>
                        <td>
                          <span className={`pill ${row.enabled === false ? "pill-muted" : "pill-good"}`}>
                            {row.enabled === false ? "отключён" : row.status === "delivered" ? "выдан" : "на складе"}
                          </span>
                        </td>
                        <td className="tnum">{row.faceValue}</td>
                        <td className="tnum">{row.remaining ?? "—"}</td>
                        <td className="tnum mprice">{row.spent ?? "—"}</td>
                        <td>
                          <a href={row.viewUrl} target="_blank" rel="noreferrer">
                            открыть
                          </a>
                        </td>
                        <td>
                          <button
                            className={`btn btn-ghost btn-sm ${row.enabled === false ? "" : "openkeys-danger"}`}
                            type="button"
                            disabled={busy}
                            onClick={() => void toggle(row)}
                          >
                            {row.enabled === false ? "Включить" : "Отключить"}
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </section>
        </div>
      </div>
    </AppShell>
  );
}
