"use client";

import { useCallback, useEffect, useState } from "react";
import {
  api,
  ApiError,
  formatBps,
  formatDate,
  formatUsd,
  formatUsdCompact,
  isPositiveNanoUsd,
  parseCanonicalNanoUsd,
  type PartnerActivityEvent,
  type PartnerAnalyticsList,
  type PartnerAnalyticsRow,
  type PartnerAnalyticsSortKey,
  type PartnerDetailBundle,
} from "@/lib/api";
import { Badge, Button, Card, EmptyState, Input, Loading, Notice, StatusBadge, Table } from "@/components/ui";
import { localeFor, useI18n } from "@/components/i18n";

// На admin.partners Caddy инжектит x-sales-admin-key после managed admin auth → ключ пустой ("").
function adminHeaders(key: string): Record<string, string> {
  return key ? { "x-sales-admin-key": key } : {};
}

const PAGE = 25;
const faint = { color: "var(--text-faint)" } as const;

type Translate = (en: string, ru: string) => string;

function relTime(iso: string | null, t: Translate, locale: string): string {
  if (!iso) return t("never", "никогда");
  const diff = Date.now() - new Date(iso).getTime();
  if (Number.isNaN(diff)) return "—";
  const d = Math.floor(diff / 86_400_000);
  if (d <= 0) {
    const h = Math.floor(diff / 3_600_000);
    if (h <= 0) return t("just now", "только что");
    return t(`${h}h ago`, `${h} ч назад`);
  }
  if (d < 30) return t(`${d}d ago`, `${d} дн назад`);
  return formatDate(iso, locale);
}

function convPct(row: PartnerAnalyticsRow): string {
  if (row.referredUsers === 0) return "—";
  return `${Math.round((row.convertedUsers / row.referredUsers) * 100)}%`;
}

function activityLabel(event: PartnerActivityEvent, t: Translate): string {
  const tail = event.label.split(" ").at(-1) ?? "";
  switch (event.type) {
    case "referral": return t(event.label, `Новый реферал ${tail}`);
    case "deposit": return t(event.label, `Пополнение реферала ${tail}`);
    case "discount_link_created": return t(event.label, `Создана старая маркерная ссылка ${String(event.meta.code ?? tail)}`);
    case "discount_link_used": return t(event.label, `Использована старая маркерная ссылка ${String(event.meta.code ?? tail)}`);
    case "promo_created": return t(event.label, `Создан промокод ${String(event.meta.code ?? tail)}`);
    case "promo_redeemed": return t(event.label, `Погашен промокод ${String(event.meta.code ?? tail)}`);
    case "payout_requested": return t(event.label, "Запрошена выплата");
    case "payout_decided": return t(event.label, `Решение по выплате: ${String(event.meta.status ?? tail)}`);
    case "login": return t(event.label, "Вход в кабинет");
    case "admin": return t(event.label, event.label.replace(/^Admin:/, "Администратор:"));
    default: return event.label;
  }
}

function columns(t: Translate): { key: PartnerAnalyticsSortKey; label: string; num?: boolean }[] {
  return [
    { key: "deposits_total", label: t("Deposits driven", "Привлечённые пополнения"), num: true },
    { key: "referred_users", label: t("Referred", "Рефералы"), num: true },
    { key: "spend_total", label: t("Spend", "Расходы"), num: true },
    { key: "earned_total", label: t("Earned", "Заработано"), num: true },
    { key: "unpaid", label: t("Unpaid", "Не выплачено"), num: true },
    { key: "team_size", label: t("Team", "Команда"), num: true },
    { key: "last_seen_at", label: t("Last seen", "Последняя активность"), num: false },
  ];
}

export function PartnersTab({ adminKey }: { adminKey: string }) {
  const { lang, t } = useI18n();
  const locale = localeFor(lang);
  const [data, setData] = useState<PartnerAnalyticsList | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sort, setSort] = useState<PartnerAnalyticsSortKey>("deposits_total");
  const [dir, setDir] = useState<"asc" | "desc">("desc");
  const [status, setStatus] = useState<"all" | "active" | "pending" | "suspended">("all");
  const [search, setSearch] = useState("");
  const [query, setQuery] = useState("");
  const [offset, setOffset] = useState(0);
  const [selected, setSelected] = useState<PartnerAnalyticsRow | null>(null);

  useEffect(() => {
    const timer = setTimeout(() => {
      setQuery(search);
      setOffset(0);
    }, 300);
    return () => clearTimeout(timer);
  }, [search]);

  const load = useCallback(async () => {
    setError(null);
    const params = new URLSearchParams({ sort, dir, status, limit: String(PAGE), offset: String(offset) });
    if (query.trim()) params.set("q", query.trim());
    try {
      const res = await api<PartnerAnalyticsList>(`/v1/admin/partner-analytics?${params.toString()}`, {
        headers: adminHeaders(adminKey),
      });
      setData(res);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("Failed to load analytics.", "Не удалось загрузить аналитику."));
    }
  }, [adminKey, sort, dir, status, query, offset, t]);

  useEffect(() => {
    void load();
  }, [load]);

  function toggleSort(key: PartnerAnalyticsSortKey) {
    if (sort === key) setDir((d) => (d === "desc" ? "asc" : "desc"));
    else {
      setSort(key);
      setDir("desc");
    }
    setOffset(0);
  }

  const totals = data?.totals;
  const items = data?.items ?? [];
  const shownFrom = totals && totals.total > 0 ? offset + 1 : 0;
  const shownTo = Math.min(offset + PAGE, totals?.total ?? 0);
  const convTotal = totals && totals.referredUsers > 0 ? Math.round((totals.convertedUsers / totals.referredUsers) * 100) : 0;

  return (
    <div className="stack">
      {/* KPI strip over the CURRENT filter */}
      <div className="stat-grid" style={{ gridTemplateColumns: "repeat(5, 1fr)" }}>
        <Kpi label={t("Partners", "Партнёры")} value={totals ? String(totals.total) : "…"} foot={totals ? t(`${totals.active} active`, `${totals.active} активных`) : ""} />
        <Kpi label={t("Deposits driven", "Привлечённые пополнения")} value={totals ? formatUsd(totals.depositsNano) : "…"} foot={t("real money in", "реальные деньги")} accent />
        <Kpi label={t("Referred users", "Привлечённые пользователи")} value={totals ? String(totals.referredUsers) : "…"} foot={t(`${totals?.convertedUsers ?? 0} deposited`, `${totals?.convertedUsers ?? 0} пополнили баланс`)} />
        <Kpi label={t("Conversion", "Конверсия")} value={totals ? `${convTotal}%` : "…"} foot={t("referred → paid", "рефералы → платящие")} />
        <Kpi label={t("Payable now", "К выплате сейчас")} value={totals ? formatUsd(totals.payableNano) : "…"} foot={t(`${totals ? formatUsd(totals.debtNano) : "…"} partner debt`, `${totals ? formatUsd(totals.debtNano) : "…"} долг партнёров`)} />
      </div>

      <Card
        title={t("Partner analytics", "Аналитика партнёров")}
        sub={t("Ranked by real deposits their referrals paid. Click a row to open the partner card with full stats and activity.", "Рейтинг по реальным пополнениям рефералов. Нажмите строку, чтобы открыть карточку партнёра со статистикой и активностью.")}
      >
        <div style={{ display: "flex", gap: 10, flexWrap: "wrap", alignItems: "center", marginBottom: 12 }}>
          <Input
            placeholder={t("Search @username, email, name, code…", "Поиск по @username, email, имени или коду…")}
            aria-label={t("Search partners", "Поиск партнёров")}
            name="partner-search"
            autoComplete="off"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            style={{ maxWidth: 320 }}
          />
          <select
            aria-label={t("Filter partners by status", "Фильтр партнёров по статусу")}
            name="partner-status"
            value={status}
            onChange={(e) => {
              setStatus(e.target.value as typeof status);
              setOffset(0);
            }}
            style={selectStyle}
          >
            <option value="all">{t("All statuses", "Все статусы")}</option>
            <option value="active">{t("Active", "Активные")}</option>
            <option value="pending">{t("Pending", "Ожидают")}</option>
            <option value="suspended">{t("Suspended", "Приостановлены")}</option>
          </select>
          <span style={{ marginLeft: "auto", fontSize: 12, ...faint }}>
            {totals ? `${shownFrom}–${shownTo} of ${totals.total}` : ""}
          </span>
          <div style={{ display: "flex", gap: 6 }}>
            <Button size="sm" variant="ghost" disabled={offset === 0} onClick={() => setOffset(Math.max(0, offset - PAGE))}>
              ‹ {t("Prev", "Назад")}
            </Button>
            <Button size="sm" variant="ghost" disabled={!totals || shownTo >= totals.total} onClick={() => setOffset(offset + PAGE)}>
              {t("Next", "Вперёд")} ›
            </Button>
          </div>
        </div>

        {error ? <Notice kind="error">{error}</Notice> : !data ? <Loading /> : items.length === 0 ? (
          <EmptyState title={t("No partners match this filter", "Нет партнёров с такими параметрами")} />
        ) : (
          <Table
            head={
              <>
                <th>{t("Partner", "Партнёр")}</th>
                {columns(t).map((c) => (
                  <SortTh key={c.key} col={c} sort={sort} dir={dir} onClick={() => toggleSort(c.key)} />
                ))}
                <th>{t("Status", "Статус")}</th>
              </>
            }
          >
            {items.map((p) => (
              <tr key={p.id}>
                <td>
                  <button className="table-row-button" type="button" onClick={() => setSelected(p)}>
                    <span style={{ fontWeight: 600 }}>{p.telegramUsername ? `@${p.telegramUsername}` : p.email ?? "—"}</span>
                    <span style={{ fontSize: 12, ...faint }}>{p.displayName ?? p.referralCode}</span>
                  </button>
                </td>
                <td className="num">
                  <div style={{ fontWeight: 600 }}>{formatUsd(p.depositsTotalNano)}</div>
                  <div style={{ fontSize: 11, ...faint }}>{formatUsd(p.deposits30dNano)} · {t("30d", "30 дн")}</div>
                </td>
                <td className="num">
                  <div>{p.referredUsers}</div>
                  <div style={{ fontSize: 11, ...faint }}>{p.convertedUsers} · {convPct(p)}</div>
                </td>
                <td className="num">{formatUsd(p.spendTotalNano)}</td>
                <td className="num">
                  <div>{formatUsd(p.netTotalNano)}</div>
                  <div style={{ fontSize: 11, ...faint }}>{formatUsd(p.net30dNano)} · {t("30d net", "чистыми за 30 дн")}</div>
                </td>
                <td className="num" style={{ fontWeight: isPositiveNanoUsd(p.payableNano) ? 600 : 400 }}>
                  {formatUsd(p.payableNano)}
                  {isPositiveNanoUsd(p.debtNano) ? <div style={{ color: "#d6455a", fontSize: 11 }}>{formatUsd(p.debtNano)} {t("debt", "долг")}</div> : null}
                </td>
                <td className="num">{p.teamSize}</td>
                <td style={{ fontSize: 12, ...faint }}>{relTime(p.lastSeenAt, t, locale)}</td>
                <td><StatusBadge status={p.status} /></td>
              </tr>
            ))}
          </Table>
        )}
      </Card>

      {selected ? (
        <PartnerDrawer
          row={selected}
          adminKey={adminKey}
          onClose={() => setSelected(null)}
          onChanged={() => {
            void load();
          }}
        />
      ) : null}
    </div>
  );
}

function SortTh({
  col,
  sort,
  dir,
  onClick,
}: {
  col: { key: PartnerAnalyticsSortKey; label: string; num?: boolean };
  sort: PartnerAnalyticsSortKey;
  dir: "asc" | "desc";
  onClick: () => void;
}) {
  const active = sort === col.key;
  return (
    <th
      className={col.num ? "num" : undefined}
      aria-sort={active ? (dir === "desc" ? "descending" : "ascending") : "none"}
    >
      <button className="sort-button" type="button" onClick={onClick}>
        {col.label} <span aria-hidden>{active ? (dir === "desc" ? "▾" : "▴") : ""}</span>
      </button>
    </th>
  );
}

function Kpi({ label, value, foot, accent }: { label: string; value: string; foot?: string; accent?: boolean }) {
  return (
    <div className="stat-card">
      <div className="stat-label">{label}</div>
      <div className="stat-value" style={accent ? { color: "var(--accent-strong, #3b5bdb)" } : undefined}>{value}</div>
      {foot ? <div className="stat-foot">{foot}</div> : null}
    </div>
  );
}

const selectStyle: React.CSSProperties = {
  padding: "8px 10px",
  borderRadius: 8,
  border: "1px solid var(--border)",
  background: "var(--surface)",
  color: "var(--text)",
  fontSize: 13,
};

// ---------------------------------------------------------------------------
// Partner detail drawer
// ---------------------------------------------------------------------------

function PartnerDrawer({
  row,
  adminKey,
  onClose,
  onChanged,
}: {
  row: PartnerAnalyticsRow;
  adminKey: string;
  onClose: () => void;
  onChanged: () => void;
}) {
  const { lang, t } = useI18n();
  const locale = localeFor(lang);
  const [detail, setDetail] = useState<PartnerDetailBundle | null>(null);
  const [activity, setActivity] = useState<PartnerActivityEvent[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [bps, setBps] = useState(String(row.commissionBps));
  const [subBps, setSubBps] = useState(String(row.subCommissionBps));

  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", closeOnEscape);
    return () => document.removeEventListener("keydown", closeOnEscape);
  }, [onClose]);

  const reload = useCallback(async () => {
    try {
      const [d, a] = await Promise.all([
        api<PartnerDetailBundle>(`/v1/admin/partners/${row.id}/analytics`, { headers: adminHeaders(adminKey) }),
        api<{ events: PartnerActivityEvent[] }>(`/v1/admin/partners/${row.id}/activity?limit=80`, { headers: adminHeaders(adminKey) }),
      ]);
      setDetail(d);
      setActivity(a.events);
      setBps(String(d.partner.commissionBps));
      setSubBps(String(d.partner.subCommissionBps));
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("Failed to load partner.", "Не удалось загрузить партнёра."));
    }
  }, [adminKey, row.id, t]);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function patch(body: Record<string, unknown>) {
    setBusy(true);
    setError(null);
    try {
      await api(`/v1/admin/partners/${row.id}`, { method: "PATCH", headers: adminHeaders(adminKey), body });
      await reload();
      onChanged();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("Update failed.", "Не удалось сохранить изменения."));
    } finally {
      setBusy(false);
    }
  }

  async function postPromo(body: { enabled: boolean; maxValueUsd: number; maxCount: number }) {
    setBusy(true);
    setError(null);
    try {
      await api(`/v1/admin/partners/${row.id}/promo`, { method: "POST", headers: adminHeaders(adminKey), body });
      await reload();
      onChanged();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("Promo update failed.", "Не удалось обновить промо-настройки."));
    } finally {
      setBusy(false);
    }
  }

  const p = detail?.partner ?? row;
  const label = p.telegramUsername ? `@${p.telegramUsername}` : p.email ?? p.id.slice(0, 8);
  const suspended = p.status !== "active";
  const dirty = bps !== String(p.commissionBps) || subBps !== String(p.subCommissionBps);

  // Granting the B2B right also fixes the ceiling: the partner may give their own customers any
  // discount up to it, and nothing beyond. Revoking clears the ceiling with it, so a stale number
  // can never read as authority the partner no longer holds.
  function editB2b() {
    const current = p.b2bEnabled ? String(p.b2bMaxDiscountBps / 100) : t("off", "выкл");
    const v = window.prompt(
      t(
        `B2B rights for ${label}\nMax discount percent this partner may give their own customers (e.g. 70), or "off".\nTheir referrals stay ordinary B2C customers unless the partner converts them.`,
        `B2B-права для ${label}\nМаксимальная скидка, которую партнёр может дать своим клиентам (например, 70), либо «выкл».\nРефералы остаются обычными B2C-клиентами, пока партнёр сам их не переведёт.`,
      ),
      current,
    );
    if (v == null) return;
    if (["off", "выкл"].includes(v.trim().toLowerCase())) return void patch({ b2bEnabled: false, b2bMaxDiscountBps: 0 });
    const m = /^\s*(\d{1,2}(?:\.\d)?)\s*%?\s*$/.exec(v);
    const percent = m ? Number(m[1]) : Number.NaN;
    if (!Number.isFinite(percent) || percent <= 0 || percent > 95) {
      return setError(t("Enter a percent between 0 and 95, or \"off\".", "Введите процент от 0 до 95 либо «выкл»."));
    }
    void patch({ b2bEnabled: true, b2bMaxDiscountBps: Math.round(percent * 100) });
  }

  function editPromo() {
    const v = window.prompt(t(
      `Promo codes for ${label}\n"maxUSD/count" to enable (e.g. 20/10), or "off".`,
      `Промокоды для ${label}\nВведите «максимумUSD/количество» (например, 20/10) либо «выкл».`,
    ), p.promoEnabled ? "" : t("off", "выкл"));
    if (v == null) return;
    if (["off", "выкл"].includes(v.trim().toLowerCase())) return void postPromo({ enabled: false, maxValueUsd: 0, maxCount: 0 });
    const m = /^\s*(\d{1,5})\s*\/\s*(\d{1,5})\s*$/.exec(v);
    if (!m) return setError(t("Format: maxUSD/count, e.g. 20/10", "Формат: максимумUSD/количество, например 20/10"));
    void postPromo({ enabled: true, maxValueUsd: Number(m[1]), maxCount: Number(m[2]) });
  }

  return (
    <div style={overlayStyle}>
      <button
        type="button"
        aria-label={t("Close partner details", "Закрыть карточку партнёра")}
        style={backdropStyle}
        onClick={onClose}
      />
      <aside style={panelStyle} role="dialog" aria-modal="true" aria-labelledby="partner-detail-title">
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start", gap: 12 }}>
          <div>
            <h2 id="partner-detail-title" style={{ fontSize: 18, fontWeight: 700 }}>{label} <StatusBadge status={p.status} /></h2>
            <div style={{ fontSize: 12, ...faint, marginTop: 2 }}>
              {p.displayName ? `${p.displayName} · ` : ""}{t("code", "код")} <span className="mono">{p.referralCode}</span>
              {p.parentLabel ? <> · {t("under", "в команде")} {p.parentLabel}</> : null}
            </div>
            <div style={{ fontSize: 12, ...faint }}>
              {t("joined", "подключён")} {formatDate(p.createdAt, locale)} · {t("last seen", "последняя активность")} {relTime(p.lastSeenAt, t, locale)}
            </div>
          </div>
          <Button size="sm" variant="ghost" onClick={onClose}>✕ {t("Close", "Закрыть")}</Button>
        </div>

        {error ? <div style={{ marginTop: 10 }}><Notice kind="error">{error}</Notice></div> : null}

        {/* Rights & actions */}
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap", alignItems: "center", marginTop: 14 }}>
          <label style={{ fontSize: 12, ...faint }}>{t("Rate", "Ставка")}</label>
          <Input className="inline-edit" inputMode="numeric" name="partner-commission-bps" autoComplete="off" value={bps} onChange={(e) => setBps(e.target.value.replace(/[^\d]/g, ""))} aria-label={t("Commission bps", "Комиссия в базисных пунктах")} style={{ width: 84 }} />
          <span style={{ fontSize: 11, ...faint }}>{/^\d+$/.test(bps) ? formatBps(Number(bps)) : "—"}</span>
          <label style={{ fontSize: 12, ...faint }}>{t("Sub", "Команда")}</label>
          <Input className="inline-edit" inputMode="numeric" name="partner-sub-commission-bps" autoComplete="off" value={subBps} onChange={(e) => setSubBps(e.target.value.replace(/[^\d]/g, ""))} aria-label={t("Sub bps", "Командная комиссия в базисных пунктах")} style={{ width: 84 }} />
          <span style={{ fontSize: 11, ...faint }}>{/^\d+$/.test(subBps) ? formatBps(Number(subBps)) : "—"}</span>
          <Button size="sm" variant="ghost" disabled={!dirty || busy || !/^\d+$/.test(bps) || !/^\d+$/.test(subBps)} onClick={() => patch({ commissionBps: Number(bps), subCommissionBps: Number(subBps) })}>{t("Save", "Сохранить")}</Button>
          <Button size="sm" variant={suspended ? "primary" : "danger"} disabled={busy} onClick={() => patch({ status: suspended ? "active" : "suspended" })}>{suspended ? t("Activate", "Активировать") : t("Suspend", "Приостановить")}</Button>
          <Button size="sm" variant="ghost" disabled={busy} onClick={editPromo}>{p.promoEnabled ? t("Promo: on", "Промо: вкл") : t("Promo: off", "Промо: выкл")}</Button>
          <Button size="sm" variant="ghost" disabled={busy} onClick={editB2b}>
            {p.b2bEnabled ? t(`B2B: up to ${formatBps(p.b2bMaxDiscountBps)}`, `B2B: до ${formatBps(p.b2bMaxDiscountBps)}`) : t("B2B: off", "B2B: выкл")}
          </Button>
          {p.referralDiscountEnabled ? (
            <Badge tone="yellow">{t("Legacy marker", "Старый маркер")} {formatBps(p.referralDiscountBps)} · {t("no price effect", "не влияет на цену")}</Badge>
          ) : null}
        </div>

        {/* Stat grid */}
        <div className="stat-grid" style={{ gridTemplateColumns: "repeat(4, 1fr)", marginTop: 16 }}>
          <Kpi label={t("Deposits driven", "Привлечённые пополнения")} value={formatUsd(p.depositsTotalNano)} foot={`${formatUsd(p.deposits30dNano)} · ${t("30d", "30 дн")}`} accent />
          <Kpi label={t("Referred / paid", "Рефералы / платящие")} value={`${p.referredUsers} / ${p.convertedUsers}`} foot={`${convPct(p)} ${t("conversion", "конверсия")}`} />
          <Kpi label={t("Real spend", "Реальные расходы")} value={formatUsd(p.spendTotalNano)} foot={`${formatUsd(p.spend30dNano)} · ${t("30d", "30 дн")}`} />
          <Kpi label={t("Net earnings", "Чистый заработок")} value={formatUsd(p.netTotalNano)} foot={`${formatUsd(p.adjustmentTotalNano)} ${t("returns", "возвраты")}`} />
          <Kpi label={t("Paid out", "Выплачено")} value={formatUsd(p.paidNano)} />
          <Kpi label={t("Payable now", "К выплате сейчас")} value={formatUsd(p.payableNano)} foot={`${formatUsd(p.debtNano)} ${t("debt", "долг")}`} />
          <Kpi label={t("Team", "Команда")} value={String(p.teamSize)} foot={t("sub-partners", "субпартнёры")} />
          <Kpi label={t("Legacy links / promos", "Старые ссылки / промо")} value={`${p.linksUsed}/${p.linksTotal} · ${p.promosUsed}/${p.promosTotal}`} foot={t("used / total", "использовано / всего")} />
        </div>

        {detail ? <Sparkline daily={detail.daily} /> : null}

        {!detail || !activity ? (
          <div style={{ marginTop: 16 }}><Loading /></div>
        ) : (
          <div style={{ marginTop: 16, display: "grid", gap: 16 }}>
            <Section title={t(`Activity (${activity.length})`, `Активность (${activity.length})`)}>
              {activity.length === 0 ? <Muted>{t("No activity yet.", "Активности пока нет.")}</Muted> : (
                <ul style={feedStyle}>
                  {activity.map((e, i) => (
                    <li key={i} style={feedItemStyle}>
                      <span style={{ ...dotStyle, background: activityColor(e.type) }} />
                      <span style={{ flex: 1 }}>{activityLabel(e, t)}{e.amountNano ? <strong> {formatUsd(e.amountNano)}</strong> : null}</span>
                      <span style={{ fontSize: 11, ...faint, whiteSpace: "nowrap" }}>{relTime(e.at, t, locale)}</span>
                    </li>
                  ))}
                </ul>
              )}
            </Section>

            <Section title={t(`Legacy marker links (${detail.discountLinks.length})`, `Старые маркерные ссылки (${detail.discountLinks.length})`)}>
              {detail.discountLinks.length === 0 ? <Muted>{t("None issued.", "Не выпускались.")}</Muted> : (
                <MiniTable rows={detail.discountLinks.map((l) => [
                  <span key="c" className="mono">{l.code}</span>,
                  formatBps(l.discountBps),
                  l.note ?? "—",
                  l.consumedAt ? t(`used ${relTime(l.consumedAt, t, locale)}`, `использовано ${relTime(l.consumedAt, t, locale)}`) : t("unused", "не использовано"),
                ])} cols={[t("Code", "Код"), t("Marker", "Маркер"), t("For", "Для"), t("State", "Состояние")]} />
              )}
            </Section>

            <Section title={t(`Promo codes (${detail.promos.length})`, `Промокоды (${detail.promos.length})`)}>
              {detail.promos.length === 0 ? <Muted>{t("None created.", "Не создавались.")}</Muted> : (
                <MiniTable rows={detail.promos.map((c) => [
                  <span key="c" className="mono">{c.code}</span>,
                  formatUsd(c.valueNano),
                  <StatusBadge key="s" status={c.status} />,
                  c.redeemedAt ? t(`redeemed ${relTime(c.redeemedAt, t, locale)}`, `погашен ${relTime(c.redeemedAt, t, locale)}`) : formatDate(c.createdAt, locale),
                ])} cols={[t("Code", "Код"), t("Value", "Номинал"), t("Status", "Статус"), t("When", "Когда")]} />
              )}
            </Section>

            <Section title={t(`Team (${detail.team.length})`, `Команда (${detail.team.length})`)}>
              {detail.team.length === 0 ? <Muted>{t("No sub-partners.", "Субпартнёров нет.")}</Muted> : (
                <MiniTable rows={detail.team.map((m) => [
                  m.telegramUsername ? `@${m.telegramUsername}` : m.email ?? m.id.slice(0, 8),
                  formatBps(m.commissionBps),
                  t(`${m.referredUsers} refs`, `${m.referredUsers} рефералов`),
                  `${formatUsd(m.myOverrideNetNano)} ${t("net override", "чистая командная комиссия")}`,
                ])} cols={[t("Sub-partner", "Субпартнёр"), t("Rate", "Ставка"), t("Referrals", "Рефералы"), t("My override", "Моя комиссия")]} />
              )}
            </Section>

            <Section title={t(`Referred users (${detail.referrals.length})`, `Привлечённые пользователи (${detail.referrals.length})`)}>
              {detail.referrals.length === 0 ? <Muted>{t("No referrals yet.", "Рефералов пока нет.")}</Muted> : (
                <MiniTable rows={detail.referrals.slice(0, 50).map((u) => [
                  <span key="u" className="mono">{u.userMask}</span>,
                  u.customerType === "b2b"
                    ? <Badge key="t" tone="green">B2B</Badge>
                    : u.customerType === "b2c"
                      ? <span key="t">B2C{u.discountPercent != null ? ` · ${t("actual", "фактически")} ${u.discountPercent}%` : ""}</span>
                      : "—",
                  formatDate(u.attributedAt, locale),
                  `${formatUsd(u.spendNano)} ${t("spend", "расходы")}`,
                  `${formatUsd(u.netNano)} ${t("net earned", "чистый заработок")}`,
                ])} cols={[t("User", "Пользователь"), t("Type", "Тип"), t("Joined", "Привлечён"), t("Spend", "Расходы"), t("Earned", "Заработано")]} />
              )}
            </Section>

            <Section title={t(`Payouts (${detail.payouts.length})`, `Выплаты (${detail.payouts.length})`)}>
              {detail.payouts.length === 0 ? <Muted>{t("No payouts.", "Выплат нет.")}</Muted> : (
                <MiniTable rows={detail.payouts.map((po) => [
                  formatUsd(po.amountNano),
                  <StatusBadge key="s" status={po.status} />,
                  formatDate(po.requestedAt, locale),
                  po.paidAt ? `${t("paid", "выплачено")} ${formatDate(po.paidAt, locale)}` : po.adminNote ?? "—",
                ])} cols={[t("Amount", "Сумма"), t("Status", "Статус"), t("Requested", "Запрошено"), t("Note", "Примечание")]} />
              )}
            </Section>
          </div>
        )}
      </aside>
    </div>
  );
}

function Sparkline({ daily }: { daily: { date: string; spendNano: string; earnedNano: string; adjustmentNano: string; netNano: string }[] }) {
  const { t } = useI18n();
  if (daily.length === 0) return null;
  const parsed = daily.map((d) => parseCanonicalNanoUsd(d.spendNano));
  if (parsed.some((value) => value === null)) {
    return <div style={{ marginTop: 14, fontSize: 11, ...faint }}>{t("Real-spend chart unavailable: invalid API money.", "График реальных расходов недоступен: API вернул некорректную денежную сумму.")}</div>;
  }
  const values = parsed as bigint[];
  const max = values.reduce((m, value) => value > m ? value : m, 1n);
  const total = values.reduce((m, value) => m + value, 0n);
  return (
    <div style={{ marginTop: 14 }}>
      <div style={{ display: "flex", justifyContent: "space-between", fontSize: 11, ...faint, marginBottom: 6 }}>
        <span>{t("Real spend · last 30 days", "Реальные расходы · последние 30 дней")}</span>
        <span>{formatUsdCompact(total.toString())} {t("total", "всего")}</span>
      </div>
      <div style={{ display: "flex", alignItems: "flex-end", gap: 2, height: 44 }}>
        {daily.map((d, index) => {
          const v = values[index];
          const h = max > 0n ? Number((v * 100n) / max) : 0;
          return (
            <div
              key={d.date}
              title={`${d.date}: ${formatUsd(d.spendNano)}`}
              style={{ flex: 1, height: `${Math.max(h, 2)}%`, background: v > 0n ? "var(--accent-strong, #3b5bdb)" : "var(--border)", borderRadius: 2, minWidth: 3 }}
            />
          );
        })}
      </div>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div>
      <div style={{ fontSize: 12, fontWeight: 700, textTransform: "uppercase", letterSpacing: "0.05em", ...faint, marginBottom: 8 }}>{title}</div>
      {children}
    </div>
  );
}

function Muted({ children }: { children: React.ReactNode }) {
  return <div style={{ fontSize: 13, ...faint }}>{children}</div>;
}

function MiniTable({ cols, rows }: { cols: string[]; rows: React.ReactNode[][] }) {
  return (
    <div style={{ overflowX: "auto" }}>
      <table style={{ width: "100%", fontSize: 13, borderCollapse: "collapse" }}>
        <thead>
          <tr style={{ textAlign: "left", ...faint, fontSize: 11 }}>
            {cols.map((c) => <th key={c} style={{ padding: "4px 8px", fontWeight: 500 }}>{c}</th>)}
          </tr>
        </thead>
        <tbody>
          {rows.map((r, i) => (
            <tr key={i} style={{ borderTop: "1px solid var(--border)" }}>
              {r.map((cell, j) => <td key={j} style={{ padding: "6px 8px" }}>{cell}</td>)}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function activityColor(type: string): string {
  if (type === "deposit") return "#26a15e";
  if (type === "referral") return "#3b5bdb";
  if (type.startsWith("payout")) return "#d69e2e";
  if (type.startsWith("discount") || type.startsWith("promo")) return "#7c3aed";
  if (type === "admin") return "#e8590c";
  return "var(--text-faint)";
}

const overlayStyle: React.CSSProperties = {
  position: "fixed",
  inset: 0,
  zIndex: 50,
  display: "flex",
  justifyContent: "flex-end",
};
const panelStyle: React.CSSProperties = {
  position: "relative",
  zIndex: 1,
  width: "min(680px, 100%)",
  height: "100%",
  overflowY: "auto",
  overscrollBehavior: "contain",
  background: "var(--surface, #fff)",
  borderLeft: "1px solid var(--border)",
  padding: "20px 22px",
  boxShadow: "-8px 0 24px rgba(0,0,0,0.12)",
};
const backdropStyle: React.CSSProperties = {
  position: "absolute",
  inset: 0,
  border: 0,
  background: "rgba(0,0,0,0.38)",
  cursor: "default",
};
const feedStyle: React.CSSProperties = { listStyle: "none", margin: 0, padding: 0, display: "grid", gap: 6, maxHeight: 320, overflowY: "auto" };
const feedItemStyle: React.CSSProperties = { display: "flex", alignItems: "center", gap: 8, fontSize: 13, padding: "3px 0" };
const dotStyle: React.CSSProperties = { width: 8, height: 8, borderRadius: "50%", flex: "0 0 auto" };
