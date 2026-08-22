"use client";

import { useCallback, useEffect, useState, type FormEvent } from "react";
import {
  api,
  ApiError,
  formatBps,
  formatDate,
  formatUsd,
  sumCanonicalNanoUsd,
  type AdminPayoutRow,
  type InviteRow,
  type PayoutListResponse,
} from "@/lib/api";
import {
  Badge,
  Brand,
  Button,
  Card,
  CopyButton,
  EmptyState,
  Field,
  Input,
  Loading,
  Notice,
  StatusBadge,
  Table,
} from "@/components/ui";
import { PartnersTab } from "./partner-analytics";
import { PayoutSendTab } from "./payout-send";
import { ThemeToggle } from "@/components/theme-toggle";
import { LanguageToggle, localeFor, useI18n } from "@/components/i18n";

const KEY_STORAGE = "sales_admin_key";

// На admin.partners Caddy инжектит x-sales-admin-key после managed admin auth → ключ не нужен (key="").
// При прямом доступе (без инжекта) оператор вводит ключ в KeyGate.
function adminHeaders(key: string): Record<string, string> {
  return key ? { "x-sales-admin-key": key } : {};
}

// ---------------------------------------------------------------------------
// Key gate
// ---------------------------------------------------------------------------

function KeyGate({ onUnlock }: { onUnlock: (key: string) => void }) {
  const { t } = useI18n();
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(e: FormEvent) {
    e.preventDefault();
    const trimmed = key.trim();
    if (!trimmed) return;
    setBusy(true);
    setError(null);
    try {
      await api("/v1/admin/overview", { headers: adminHeaders(trimmed) });
      sessionStorage.setItem(KEY_STORAGE, trimmed);
      onUnlock(trimmed);
    } catch (err) {
      setError(
        err instanceof ApiError && (err.status === 401 || err.status === 403)
          ? t("Invalid admin key.", "Неверный ключ администратора.")
          : err instanceof ApiError
            ? err.message
            : t("Could not reach the API.", "Не удалось подключиться к API."),
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <main className="auth-shell">
      <div className="auth-header">
        <Brand />
        <div className="gate-tools">
          <LanguageToggle />
          <ThemeToggle />
        </div>
      </div>
      <div className="auth-card">
        <h1>{t("Admin access", "Доступ администратора")}</h1>
        <p className="auth-sub">
          {t(
            "Enter the admin key. It is kept in this tab's session only.",
            "Введите ключ администратора. Он хранится только в сессии этой вкладки.",
          )}
        </p>
        {error ? <Notice kind="error">{error}</Notice> : null}
        <form onSubmit={submit}>
          <Field label={t("Admin key", "Ключ администратора")} htmlFor="sales-admin-key">
            <Input
              id="sales-admin-key"
              type="password"
              value={key}
              onChange={(e) => setKey(e.target.value)}
              placeholder="x-admin-key…"
              autoComplete="off"
              spellCheck={false}
              required
            />
          </Field>
          <Button type="submit" loading={busy} style={{ width: "100%" }}>
            {t("Enter admin key", "Войти с ключом")}
          </Button>
        </form>
      </div>
    </main>
  );
}

// ---------------------------------------------------------------------------
// Overview tab — renders whatever totals the API returns, defensively.
// ---------------------------------------------------------------------------

type Translate = (en: string, ru: string) => string;

function labelize(key: string, t: Translate): string {
  const known: Record<string, string> = {
    partners: t("Partners", "Партнёры"),
    activePartners: t("Active partners", "Активные партнёры"),
    referredUsers: t("Referred users", "Привлечённые пользователи"),
    totalSpendNano: t("Total spend", "Общий расход"),
    totalCommissionsNano: t("Total commissions", "Все комиссии"),
    totalAdjustmentsNano: t("Refund adjustments", "Корректировки возвратов"),
    totalNetCommissionsNano: t("Net commissions", "Чистые комиссии"),
    totalDebtNano: t("Partner debt", "Долг партнёров"),
    totalPayableNano: t("Payable now", "К выплате сейчас"),
    pendingPayoutsNano: t("Pending payouts", "Ожидающие выплаты"),
    paidPayoutsNano: t("Paid payouts", "Выплачено"),
  };
  if (known[key]) return known[key];
  return key
    .replace(/Nano$/, "")
    .replace(/([a-z])([A-Z])/g, "$1 $2")
    .replace(/^./, (c) => c.toUpperCase());
}

function OverviewTab({ adminKey }: { adminKey: string }) {
  const { t } = useI18n();
  const [data, setData] = useState<Record<string, unknown> | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const res = await api<Record<string, unknown>>("/v1/admin/overview", {
          headers: adminHeaders(adminKey),
        });
        if (!cancelled) setData(res);
      } catch (err) {
        if (!cancelled)
          setError(err instanceof ApiError ? err.message : t("Failed to load overview.", "Не удалось загрузить обзор."));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [adminKey, t]);

  if (error) return <Notice kind="error">{error}</Notice>;
  if (!data) return <Loading />;

  const flat: Array<{ key: string; value: string }> = [];
  const walk = (obj: Record<string, unknown>, prefix: string) => {
    for (const [k, v] of Object.entries(obj)) {
      if (v !== null && typeof v === "object" && !Array.isArray(v)) {
        walk(v as Record<string, unknown>, prefix ? `${prefix} · ${labelize(k, t)}` : labelize(k, t));
      } else if (typeof v === "string" && /Nano$/.test(k)) {
        flat.push({ key: prefix ? `${prefix} · ${labelize(k, t)}` : labelize(k, t), value: formatUsd(v) });
      } else if (typeof v === "number" || typeof v === "string") {
        flat.push({
          key: prefix ? `${prefix} · ${labelize(k, t)}` : labelize(k, t),
          value: String(v),
        });
      }
    }
  };
  walk(data, "");

  if (flat.length === 0) return <EmptyState title={t("No overview data", "Нет данных для обзора")} />;

  return (
    <div className="stat-grid" style={{ gridTemplateColumns: "repeat(3, 1fr)" }}>
      {flat.map((s) => (
        <div className="stat-card" key={s.key}>
          <div className="stat-label">{s.key}</div>
          <div className="stat-value" style={{ fontSize: 20 }}>
            {s.value}
          </div>
        </div>
      ))}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Partners tab
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Payouts tab
// ---------------------------------------------------------------------------

function PayoutRowView({
  payout,
  adminKey,
  onDone,
  onError,
}: {
  payout: AdminPayoutRow;
  adminKey: string;
  onDone: () => void;
  onError: (msg: string) => void;
}) {
  const { lang, t } = useI18n();
  const locale = localeFor(lang);
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);

  async function decide(action: string) {
    setBusy(true);
    try {
      await api(`/v1/admin/payouts/${payout.id}/decision`, {
        method: "POST",
        headers: adminHeaders(adminKey),
        body: { action, ...(note.trim() ? { note: note.trim() } : {}) },
      });
      onDone();
    } catch (err) {
      onError(err instanceof ApiError ? err.message : t("Decision failed.", "Не удалось сохранить решение."));
    } finally {
      setBusy(false);
    }
  }

  const pending = ["pending", "requested", "processing"].includes(
    payout.status.toLowerCase(),
  );

  return (
    <tr>
      <td>
        <div style={{ fontWeight: 600 }}>{payout.partnerEmail ?? payout.partnerId ?? "—"}</div>
        <div style={{ fontSize: 12, color: "var(--text-faint)" }}>
          {formatDate(payout.requestedAt, locale)}
        </div>
      </td>
      <td className="num" style={{ fontWeight: 700 }}>
        {formatUsd(payout.amountNano)}
      </td>
      <td>
        <div>{payout.method}</div>
        {payout.details ? (
          <div className="mono" style={{ fontSize: 12, color: "var(--text-faint)", wordBreak: "break-all" }}>
            {payout.details}
          </div>
        ) : null}
      </td>
      <td>
        <StatusBadge status={payout.status} />
      </td>
      <td>
        {pending ? (
          <div style={{ display: "flex", flexDirection: "column", gap: 6, minWidth: 200 }}>
            <Input
              placeholder={t("Note (optional)…", "Примечание (необязательно)…")}
              aria-label={t("Payout decision note", "Примечание к решению по выплате")}
              name={`payout-note-${payout.id}`}
              autoComplete="off"
              value={note}
              onChange={(e) => setNote(e.target.value)}
              style={{ padding: "5px 8px", fontSize: 13 }}
            />
            <div className="row-actions">
              <Button size="sm" disabled={busy} onClick={() => decide("approve")}>
                {t("Approve", "Одобрить")}
              </Button>
              <Button size="sm" variant="ghost" disabled={busy} onClick={() => decide("paid")}>
                {t("Mark paid", "Отметить выплаченной")}
              </Button>
              <Button size="sm" variant="danger" disabled={busy} onClick={() => decide("reject")}>
                {t("Reject", "Отклонить")}
              </Button>
            </div>
          </div>
        ) : payout.status.toLowerCase() === "approved" ? (
          <Button size="sm" variant="ghost" disabled={busy} onClick={() => decide("paid")}>
            {t("Mark paid", "Отметить выплаченной")}
          </Button>
        ) : (
          <span style={{ color: "var(--text-faint)", fontSize: 13 }}>
            {payout.paidAt ? `${t("Paid", "Выплачено")} ${formatDate(payout.paidAt, locale)}` : "—"}
          </span>
        )}
      </td>
    </tr>
  );
}

function PayoutsTab({ adminKey }: { adminKey: string }) {
  const { t } = useI18n();
  const [filter, setFilter] = useState<"pending" | "all">("pending");
  const [items, setItems] = useState<AdminPayoutRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setItems(null);
    try {
      // Бэкенд принимает только requested|approved|paid|rejected — "pending" даёт 422. Очередь на
      // действие = requested (только что запрошенные выплаты).
      const qs = filter === "pending" ? "?status=requested" : "";
      const res = await api<{ items: AdminPayoutRow[] }>(`/v1/admin/payouts${qs}`, {
        headers: adminHeaders(adminKey),
      });
      setItems(res.items);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("Failed to load payouts.", "Не удалось загрузить выплаты."));
    }
  }, [adminKey, filter, t]);

  useEffect(() => {
    void load();
  }, [load]);

  return (
    <>
      <div style={{ marginBottom: 16, display: "flex", gap: 8 }}>
        <Button
          size="sm"
          variant={filter === "pending" ? "primary" : "ghost"}
          onClick={() => setFilter("pending")}
        >
          {t("Pending queue", "Очередь выплат")}
        </Button>
        <Button
          size="sm"
          variant={filter === "all" ? "primary" : "ghost"}
          onClick={() => setFilter("all")}
        >
          {t("All payouts", "Все выплаты")}
        </Button>
      </div>
      {error ? <Notice kind="error">{error}</Notice> : null}
      {!items && !error ? (
        <Loading />
      ) : items && items.length === 0 ? (
        <Card>
          <EmptyState title={filter === "pending" ? t("Queue is empty", "Очередь пуста") : t("No payouts yet", "Выплат пока нет")} />
        </Card>
      ) : items ? (
        <Table
          label={t("Payout requests", "Запросы на выплаты")}
          head={
            <>
              <th>{t("Partner", "Партнёр")}</th>
              <th className="num">{t("Amount", "Сумма")}</th>
              <th>{t("Method / details", "Метод / реквизиты")}</th>
              <th>{t("Status", "Статус")}</th>
              <th>{t("Decision", "Решение")}</th>
            </>
          }
        >
          {items.map((p) => (
            <PayoutRowView
              key={p.id}
              payout={p}
              adminKey={adminKey}
              onDone={load}
              onError={setError}
            />
          ))}
        </Table>
      ) : null}
    </>
  );
}

// ---------------------------------------------------------------------------
// Onboarding tab — корневые инвайты, привязанные к telegram-юзернейму
// ---------------------------------------------------------------------------

type AdminApplicationRow = {
  id: string;
  telegramUsername: string | null;
  displayName: string | null;
  note: string | null;
  status: string;
  adminNote: string | null;
  createdAt: string;
  decidedAt: string | null;
};

function ApplicationRowView({
  application,
  adminKey,
  onDone,
  onError,
}: {
  application: AdminApplicationRow;
  adminKey: string;
  onDone: () => void;
  onError: (msg: string) => void;
}) {
  const { lang, t } = useI18n();
  const locale = localeFor(lang);
  const [bps, setBps] = useState("");
  const [subBps, setSubBps] = useState("");
  const [busy, setBusy] = useState(false);

  async function decide(action: "approve" | "reject") {
    setBusy(true);
    try {
      await api(`/v1/admin/applications/${application.id}/decision`, {
        method: "POST",
        headers: adminHeaders(adminKey),
        body: {
          action,
          ...(/^\d+$/.test(bps) ? { commissionBps: Number(bps) } : {}),
          ...(/^\d+$/.test(subBps) ? { subCommissionBps: Number(subBps) } : {}),
        },
      });
      onDone();
    } catch (err) {
      onError(err instanceof ApiError ? err.message : t("Decision failed.", "Не удалось сохранить решение."));
      setBusy(false);
    }
  }

  return (
    <tr>
      <td>
        <div style={{ fontWeight: 600 }}>
          {application.telegramUsername ? `@${application.telegramUsername}` : "—"}
        </div>
        {application.displayName ? (
          <div style={{ fontSize: 12, color: "var(--text-faint)" }}>{application.displayName}</div>
        ) : null}
      </td>
      <td style={{ maxWidth: 320, whiteSpace: "pre-wrap" }}>{application.note ?? "—"}</td>
      <td>{formatDate(application.createdAt, locale)}</td>
      <td>
        <div className="row-actions">
          <Input
            className="inline-edit"
            inputMode="numeric"
            name="applicationCommissionBps"
            autoComplete="off"
            value={bps}
            onChange={(e) => setBps(e.target.value.replace(/[^\d]/g, ""))}
            placeholder="bps…"
            aria-label={t("Commission bps", "Комиссия в базисных пунктах")}
            style={{ maxWidth: 90 }}
          />
          <Input
            className="inline-edit"
            inputMode="numeric"
            name="applicationSubCommissionBps"
            autoComplete="off"
            value={subBps}
            onChange={(e) => setSubBps(e.target.value.replace(/[^\d]/g, ""))}
            placeholder="sub…"
            aria-label={t("Sub-commission bps", "Командная комиссия в базисных пунктах")}
            style={{ maxWidth: 90 }}
          />
          <Button size="sm" disabled={busy} onClick={() => decide("approve")}>
            {t("Approve", "Одобрить")}
          </Button>
          <Button size="sm" variant="danger" disabled={busy} onClick={() => decide("reject")}>
            {t("Reject", "Отклонить")}
          </Button>
        </div>
      </td>
    </tr>
  );
}

function ApplicationsCard({ adminKey }: { adminKey: string }) {
  const { t } = useI18n();
  const [items, setItems] = useState<AdminApplicationRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const res = await api<{ items: AdminApplicationRow[] }>("/v1/admin/applications?status=pending", {
        headers: adminHeaders(adminKey),
      });
      setItems(res.items);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("Failed to load applications.", "Не удалось загрузить заявки."));
    }
  }, [adminKey, t]);

  useEffect(() => {
    void load();
  }, [load]);

  return (
    <Card
      title={t("Applications", "Заявки")}
      sub={t(
        "People who signed in with Telegram without an invite and applied. Approve creates the partner account instantly.",
        "Пользователи, которые вошли через Telegram без приглашения и отправили заявку. Одобрение сразу создаёт партнёрский аккаунт.",
      )}
    >
      {error ? <Notice kind="error">{error}</Notice> : null}
      {!items ? (
        <Loading />
      ) : items.length === 0 ? (
        <EmptyState title={t("No pending applications", "Нет заявок на рассмотрении")} />
      ) : (
        <Table
          label={t("Partner applications", "Заявки партнёров")}
          head={
            <>
              <th>Telegram</th>
              <th>{t("Application", "Заявка")}</th>
              <th>{t("Submitted", "Отправлена")}</th>
              <th>{t("Decision (bps optional)", "Решение (ставки необязательны)")}</th>
            </>
          }
        >
          {items.map((application) => (
            <ApplicationRowView
              key={application.id}
              application={application}
              adminKey={adminKey}
              onDone={load}
              onError={setError}
            />
          ))}
        </Table>
      )}
    </Card>
  );
}

// "10" / "12.5" (percent) -> bps (1250). Пусто -> undefined (омит, сервер подставит дефолт),
// заполнено но не число -> null (ошибка валидации).
function pctToBpsOptional(value: string): number | null | undefined {
  const s = value.trim();
  if (s === "") return undefined;
  if (!/^\d{1,3}(\.\d{1,2})?$/.test(s)) return null;
  const pct = Number(s);
  if (!Number.isFinite(pct)) return null;
  return Math.round(pct * 100);
}

function OnboardingTab({ adminKey }: { adminKey: string }) {
  const { lang, t } = useI18n();
  const locale = localeFor(lang);
  const [items, setItems] = useState<InviteRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [username, setUsername] = useState("");
  const [commissionPct, setCommissionPct] = useState("");
  const [subPct, setSubPct] = useState("");
  // Empty = no B2B right. A number grants it and fixes the ceiling in one step.
  const [b2bMaxPct, setB2bMaxPct] = useState("");
  const [busy, setBusy] = useState(false);
  const [created, setCreated] = useState<{ inviteUrl: string; telegramUsername: string | null } | null>(null);

  const load = useCallback(async () => {
    try {
      const res = await api<{ items: InviteRow[] }>("/v1/admin/invites", {
        headers: adminHeaders(adminKey),
      });
      setItems(res.items);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("Failed to load invites.", "Не удалось загрузить приглашения."));
    }
  }, [adminKey, t]);

  useEffect(() => {
    void load();
  }, [load]);

  async function create() {
    setError(null); // очищаем прошлую ошибку сразу при клике, чтобы не висела на валидном вводе
    const clean = username.trim().replace(/^@/, "");
    if (!/^[A-Za-z0-9_]{5,32}$/.test(clean)) {
      setError(t(
        "Enter the sales partner's Telegram username (5–32 letters, digits, underscore).",
        "Введите Telegram-имя партнёра (5–32 латинских букв, цифр или символов подчёркивания).",
      ));
      return;
    }
    // Пустое поле процента = «по умолчанию» (не отправляем — сервер подставит дефолт). Заполненное,
    // но не число → ошибка. undefined = омит, null = невалидно.
    const commissionBps = pctToBpsOptional(commissionPct);
    const subCommissionBps = pctToBpsOptional(subPct);
    if (commissionBps === null || subCommissionBps === null) {
      setError(t("Percents must be numbers like 10 or 12.5.", "Проценты должны быть числами, например 10 или 12.5."));
      return;
    }
    if ((commissionBps ?? 0) > 10000 || (subCommissionBps ?? 0) > 10000) {
      setError(t("Commission percent cannot exceed 100%.", "Комиссия не может превышать 100%."));
      return;
    }
    const b2bMaxBps = pctToBpsOptional(b2bMaxPct);
    if (b2bMaxBps === null) {
      setError(t("B2B max discount must be a number like 70 or 72.5.", "Максимальная B2B-скидка должна быть числом, например 70 или 72.5."));
      return;
    }
    if ((b2bMaxBps ?? 0) > 9500) {
      setError(t("B2B max discount cannot exceed 95%.", "Максимальная B2B-скидка не может превышать 95%."));
      return;
    }
    setBusy(true);
    try {
      const res = await api<{ inviteUrl: string; telegramUsername: string | null }>("/v1/admin/invites", {
        method: "POST",
        headers: adminHeaders(adminKey),
        body: {
          telegramUsername: clean,
          ...(commissionBps !== undefined ? { commissionBps } : {}),
          ...(subCommissionBps !== undefined ? { subCommissionBps } : {}),
          // The right and its ceiling travel together: a blank field onboards an ordinary partner
          // whose referrals are plain B2C customers.
          ...(b2bMaxBps !== undefined && b2bMaxBps > 0
            ? { b2bEnabled: true, b2bMaxDiscountBps: b2bMaxBps }
            : { b2bEnabled: false }),
        },
      });
      setCreated(res);
      setUsername("");
      setB2bMaxPct("");
      void load();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("Could not create the invite.", "Не удалось создать приглашение."));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="stack">
      <ApplicationsCard adminKey={adminKey} />
      <Card
        title={t("Onboard a sales partner", "Подключить партнёра")}
        sub={t(
          "Invite is bound to their Telegram username. Send them the link — they sign in with Telegram and the account is created with the terms below.",
          "Приглашение привязано к Telegram-имени. Отправьте ссылку: партнёр войдёт через Telegram, и аккаунт создастся с указанными условиями.",
        )}
      >
        {error ? <Notice kind="error">{error}</Notice> : null}
        {created ? (
          <div style={{ marginBottom: 14 }}>
            <div className="reflink-row">
              <Input readOnly value={created.inviteUrl} aria-label={t("Created invite link", "Созданная ссылка-приглашение")} onFocus={(e) => e.currentTarget.select()} />
              <CopyButton value={created.inviteUrl} label={t("Copy invite", "Копировать приглашение")} />
            </div>
            <p className="field-hint" style={{ marginTop: 8 }}>
              {t("For", "Для")} <span className="mono">@{created.telegramUsername}</span>
            </p>
          </div>
        ) : null}
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(150px, 1fr))", gap: 12 }}>
          <Field label={t("Telegram username", "Имя пользователя Telegram")} htmlFor="root-invite-telegram">
            <Input
              id="root-invite-telegram"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              placeholder="@telegram_username…"
              autoComplete="off"
              spellCheck={false}
            />
          </Field>
          <Field label={t("Commission %", "Комиссия %")} htmlFor="root-invite-commission" hint={t("Partner's cut of referral spend", "Доля партнёра от расходов рефералов")}>
            <Input
              id="root-invite-commission"
              value={commissionPct}
              onChange={(e) => setCommissionPct(e.target.value.replace(/[^\d.]/g, ""))}
              inputMode="decimal"
              autoComplete="off"
              placeholder="10…"
            />
          </Field>
          <Field label={t("Sub-partner %", "Команда %")} htmlFor="root-invite-sub-commission" hint={t("Override on recruited sub-partners", "Доля от комиссии приглашённых партнёров")}>
            <Input
              id="root-invite-sub-commission"
              value={subPct}
              onChange={(e) => setSubPct(e.target.value.replace(/[^\d.]/g, ""))}
              inputMode="decimal"
              autoComplete="off"
              placeholder="10…"
            />
          </Field>
          <Field label={t("B2B max discount %", "Максимальная B2B-скидка %")} htmlFor="root-invite-b2b-max" hint={t("Blank = no B2B right; their referrals stay B2C", "Пусто = без B2B-права; рефералы остаются B2C")}>
            <Input
              id="root-invite-b2b-max"
              value={b2bMaxPct}
              onChange={(e) => setB2bMaxPct(e.target.value.replace(/[^\d.]/g, ""))}
              inputMode="decimal"
              autoComplete="off"
              placeholder={t("Off…", "Выкл…")}
            />
          </Field>
        </div>
        <div className="row-actions" style={{ marginTop: 14, alignItems: "center", gap: 12 }}>
          <Button onClick={create} loading={busy}>
            {t("Create invite", "Создать приглашение")}
          </Button>
          <span className="field-hint">
            {Number(b2bMaxPct || "0") > 0
              ? t(`B2B: may discount their own customers up to ${b2bMaxPct}%.`, `B2B: может давать своим клиентам скидку до ${b2bMaxPct}%.`)
              : t("B2B: off (referrals are ordinary B2C customers).", "B2B: выключено (рефералы остаются обычными B2C-клиентами).")}
          </span>
        </div>
      </Card>

      <Card title={t("Root invites", "Корневые приглашения")}>
        {!items ? (
          <Loading />
        ) : items.length === 0 ? (
          <EmptyState title={t("No invites yet", "Приглашений пока нет")} />
        ) : (
          <Table
            label={t("Root invitations", "Корневые приглашения")}
            head={
              <>
                <th>{t("For", "Для")}</th>
                <th>{t("Commission", "Комиссия")}</th>
                <th>{t("Sub", "Команда")}</th>
                <th>{t("Expires", "Истекает")}</th>
                <th>{t("Status", "Статус")}</th>
                <th />
              </>
            }
          >
            {items.map((inv) => (
              <tr key={inv.code}>
                <td className="mono">{inv.telegramUsername ? `@${inv.telegramUsername}` : "—"}</td>
                <td>{inv.commissionBps != null ? formatBps(inv.commissionBps) : t("default", "по умолчанию")}</td>
                <td>{inv.subCommissionBps != null ? formatBps(inv.subCommissionBps) : t("default", "по умолчанию")}</td>
                <td>{inv.expiresAt ? formatDate(inv.expiresAt, locale) : "—"}</td>
                <td>
                  {inv.consumedAt ? (
                    <Badge tone="green">{t("Used", "Использовано")} {formatDate(inv.consumedAt, locale)}</Badge>
                  ) : (
                    <Badge tone="yellow">{t("Unused", "Не использовано")}</Badge>
                  )}
                </td>
                <td>{!inv.consumedAt ? <CopyButton value={inv.inviteUrl} label={t("Copy", "Копировать")} /> : null}</td>
              </tr>
            ))}
          </Table>
        )}
      </Card>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------

function PayoutListTab({ adminKey }: { adminKey: string }) {
  const { lang, t } = useI18n();
  const [data, setData] = useState<PayoutListResponse | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api<PayoutListResponse>("/v1/admin/payout-list", { headers: adminHeaders(adminKey) })
      .then(setData)
      .catch((err) => setError(err instanceof ApiError ? err.message : t("Failed to load the payout list.", "Не удалось загрузить список выплат.")));
  }, [adminKey, t]);

  if (error) return <Notice kind="error">{error}</Notice>;
  if (!data) return <Loading />;

  const eligible = data.items.filter((i) => i.eligible);
  const eligibleTotal = sumCanonicalNanoUsd(eligible.map((i) => i.payableNano));
  const heldTotal = sumCanonicalNanoUsd(data.items.filter((i) => !i.eligible).map((i) => i.payableNano));
  const unpaidTotal = sumCanonicalNanoUsd(data.items.map((i) => i.payableNano));
  const win = data.period;
  const reasonLabel: Record<string, string> = {
    ok: t("Ready", "Готово"),
    no_wallet: t("No wallet", "Нет кошелька"),
    below_minimum: t("Below minimum", "Ниже минимума"),
    inactive: t("Suspended — held", "Приостановлен — удержано"),
    zero: "—",
  };
  const dt = (iso: string) => new Date(iso).toLocaleDateString(localeFor(lang), { month: "short", day: "numeric", year: "numeric" });

  return (
    <div className="stack">
      <Card
        title={t(`Payout list — period ${win.key}`, `Список выплат — период ${win.key}`)}
        sub={t(
          `Auto-generated from gross commissions before ${dt(win.end)}, signed refund adjustments and committed payouts. Window ${dt(win.payoutWindowStart)} → ${dt(win.payoutWindowEnd)} · phase: ${win.phase}. Debt is shown separately and future earnings repay it first.`,
          `Сформировано из валовой комиссии до ${dt(win.end)}, возвратных корректировок со знаком и зафиксированных выплат. Окно ${dt(win.payoutWindowStart)} → ${dt(win.payoutWindowEnd)} · этап: ${win.phase}. Долг показан отдельно и сначала погашается будущими начислениями.`,
        )}
      >
        <div className="stat-grid" style={{ gridTemplateColumns: "repeat(3, 1fr)", marginBottom: 0 }}>
          <div className="stat-card">
            <div className="stat-label">{t("Ready to pay", "Готово к выплате")}</div>
            <div className="stat-value green">{formatUsd(eligibleTotal)}</div>
            <div className="stat-foot">{t(`${eligible.length} partners eligible`, `${eligible.length} партнёров допущено`)}</div>
          </div>
          <div className="stat-card">
            <div className="stat-label">{t("Held (rolls over)", "Удержано (переносится)")}</div>
            <div className="stat-value">
              {formatUsd(heldTotal)}
            </div>
            <div className="stat-foot">{t(`${data.items.filter((i) => !i.eligible).length} not eligible yet`, `${data.items.filter((i) => !i.eligible).length} пока не допущено`)}</div>
          </div>
          <div className="stat-card">
            <div className="stat-label">{t("Total payable", "Всего к выплате")}</div>
            <div className="stat-value">
              {formatUsd(unpaidTotal)}
            </div>
            <div className="stat-foot">{t(`${data.items.length} partners with a balance`, `${data.items.length} партнёров с балансом`)}</div>
          </div>
        </div>
      </Card>

      <Card title={t("Due this window", "К выплате в этом окне")}>
        {data.items.length === 0 ? (
          <EmptyState title={t("Nothing due", "Нет выплат")}>{t("No partner has an unpaid balance for this period.", "В этом периоде ни у одного партнёра нет невыплаченного баланса.")}</EmptyState>
        ) : (
          <Table
            label={t("Payouts due this window", "Выплаты в этом окне")}
            head={
              <>
                <th>{t("Partner", "Партнёр")}</th>
                <th className="num">{t("Payable", "К выплате")}</th>
                <th>{t("Wallet (BSC)", "Кошелёк (BSC)")}</th>
                <th>{t("Status", "Статус")}</th>
              </>
            }
          >
            {data.items.map((row) => {
              const debtNano = /^\d+$/.test(row.debtNano) ? BigInt(row.debtNano) : 0n;
              return <tr key={row.partnerId}>
                <td>
                  <div style={{ fontWeight: 600 }}>
                    <span translate="no">{row.email ?? row.displayName ?? (row.telegramUsername ? `@${row.telegramUsername}` : row.partnerId.slice(0, 8))}</span>
                  </div>
                </td>
                <td className="num" style={{ fontWeight: 700 }}>
                  {formatUsd(row.payableNano)}
                  {debtNano > 0n ? <div style={{ color: "#d6455a", fontSize: 11 }}>{formatUsd(row.debtNano)} {t("debt", "долг")}</div> : null}
                </td>
                <td className="mono">{row.walletAddress ? `${row.walletAddress.slice(0, 8)}…${row.walletAddress.slice(-6)}` : "—"}</td>
                <td>
                  {row.eligible ? (
                    <Badge tone="green">{t("Ready", "Готово")}</Badge>
                  ) : (
                    <Badge tone="yellow">{reasonLabel[row.reason] ?? row.reason}</Badge>
                  )}
                </td>
              </tr>;
            })}
          </Table>
        )}
      </Card>
      <p className="field-hint">
        {t(
          "This list is the read-only period preview. Prepare, verify, and execute the on-chain batch in",
          "Это предварительный список периода только для чтения. Подготовьте, проверьте и выполните пакет в разделе",
        )}
        <strong> {t("Send payouts", "Отправка выплат")}</strong>; {t(
          "the server revalidates every partner, wallet, amount, balance, window, and hot-wallet identity immediately before signing.",
          "непосредственно перед подписью сервер повторно проверяет каждого партнёра, кошелёк, сумму, баланс, окно и идентичность горячего кошелька.",
        )}
      </p>
    </div>
  );
}

type Tab = "overview" | "onboarding" | "partners" | "payoutList" | "payouts" | "send";

export default function AdminPage() {
  const { t } = useI18n();
  const [adminKey, setAdminKey] = useState<string | null>(null);
  const [ready, setReady] = useState(false);
  const [tab, setTab] = useState<Tab>("overview");

  useEffect(() => {
    (async () => {
      // admin.partners: Caddy инжектит ключ после managed admin auth — пробуем без ключа.
      try {
        await api("/v1/admin/overview");
        setAdminKey("");
        setReady(true);
        return;
      } catch {
        // нет инжекта (прямой доступ) → ключ из сессии или KeyGate
      }
      setAdminKey(sessionStorage.getItem(KEY_STORAGE));
      setReady(true);
    })();
  }, []);

  if (!ready) return <Loading />;
  if (adminKey === null) return <KeyGate onUnlock={setAdminKey} />;

  return (
    <main className="admin-shell">
      <div className="admin-topbar">
        <div className="brand">
          <span>
            APIToken <em>Partners</em>&nbsp;
            <Badge tone="yellow">Admin</Badge>
          </span>
        </div>
        <div className="gate-tools">
          <LanguageToggle />
          <ThemeToggle />
          <Button
            size="sm"
            variant="ghost"
            onClick={() => {
              sessionStorage.removeItem(KEY_STORAGE);
              setAdminKey(null);
            }}
          >
            {t("Lock", "Заблокировать")}
          </Button>
        </div>
      </div>

      <div className="tabs" role="tablist">
        {(
          [
            ["overview", t("Overview", "Обзор")],
            ["onboarding", t("Onboarding", "Подключение")],
            ["partners", t("Partners", "Партнёры")],
            ["payoutList", t("Payout list", "Список выплат")],
            ["send", t("Send payouts", "Отправка выплат")],
            ["payouts", t("Payouts", "Выплаты")],
          ] as Array<[Tab, string]>
        ).map(([id, label]) => (
          <button
            type="button"
            key={id}
            role="tab"
            aria-selected={tab === id}
            className={`tab${tab === id ? " active" : ""}`}
            onClick={() => setTab(id)}
          >
            {label}
          </button>
        ))}
      </div>

      {tab === "overview" ? <OverviewTab adminKey={adminKey} /> : null}
      {tab === "onboarding" ? <OnboardingTab adminKey={adminKey} /> : null}
      {tab === "partners" ? <PartnersTab adminKey={adminKey} /> : null}
      {tab === "payoutList" ? <PayoutListTab adminKey={adminKey} /> : null}
      {tab === "send" ? <PayoutSendTab adminKey={adminKey} /> : null}
      {tab === "payouts" ? <PayoutsTab adminKey={adminKey} /> : null}
    </main>
  );
}
