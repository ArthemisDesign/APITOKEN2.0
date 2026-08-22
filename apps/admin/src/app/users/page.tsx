"use client";

// Пользователи — порт 1:1 функций users()/renderUsers()/bindUserPage()/usersCsv()/
// creditUser()/userAction() из crates/server/src/admin-panel.js (строки 637-730).
// Серверный поиск и пагинация /admin/users (limit 50), фильтры q/status/auth,
// сортировки — ровно USER_SORTS. Обновления приходят по resource-specific SSE.
import {
  memo,
  startTransition,
  useCallback,
  useEffect,
  useState,
  type FormEvent,
  type ReactElement,
} from "react";
import { send } from "@/lib/api";
import { compactOverviewUrl } from "@/lib/engine-urls";
import { useResources } from "@/lib/resources";
import { toast } from "@/lib/toast";
import { dialog } from "@/lib/dialog";
import { csvDate, downloadCsv } from "@/lib/csv";
import { ago, count, formatDate, money } from "@/lib/format";
import type { CommerceDashboard, EngineOverview } from "@/lib/types";
import { CardGrid, Dot, EmptyRow, LoadingGrid, PageHead, Pill, SectionHeader, StatCard, TableCard } from "@/components/ui";
import { useSpendStatsModal } from "@/components/spend-stats-modal";
import { BusinessConversionDialog, type BusinessConversionTarget } from "./business-conversion-dialog";
import { PartnerOnboardingDialog, type PartnerOnboardingTarget } from "../partners/partner-onboarding-form";
import { useI18n } from "@/lib/i18n";
import {
  clampedOffset,
  buildUsersCsvRows,
  formatUserProviderNano,
  INITIAL_USER_PAGE,
  PANEL_REASON,
  tierLabel,
  USER_ACTION_LABELS,
  USER_SORTS,
  USERS_CSV_HEADER,
  userProviderRails,
  usersQuery,
  type AdminUser,
  type AdminUsersPage,
  type EngineDemand,
  type UserAction,
  type UserActionResult,
  type UserPageState,
  type UserSortKey,
} from "./users-lib";

export const GIFT_CREDIT_REASON = "admin panel gift credit (not an external payment)";

type UsersOverview = EngineOverview & { demand?: EngineDemand };

const show = (value: number | null | undefined): number | "—" => value ?? "—";

export function UserProviderSpend({ user }: { user: AdminUser }): ReactElement {
  const rails = userProviderRails(user.provider_spend_30d);
  return (
    <td className="user-provider-cell">
      <div className="user-provider-stack">
        {rails.map((provider) => {
          const positive = provider.amountNano !== null && provider.amountNano !== "0";
          const value = !provider.available
            ? "данные недоступны"
            : positive ? formatUserProviderNano(provider.amountNano) : "расхода не было";
          return (
            <div
              key={provider.id}
              className={`user-provider-line ${provider.className}${positive ? " positive" : ""}`}
              role="img"
              aria-label={`${provider.label}: ${value}`}
              title={`${provider.label}: ${value}`}
            >
              <span className="user-provider-label">{provider.label}</span>
              <span className="user-provider-rail" aria-hidden="true">
                <i style={{ width: `${provider.shareBp / 100}%` }} />
              </span>
              <b className={!provider.available ? "unavailable" : positive ? "" : "zero"}>
                {positive ? formatUserProviderNano(provider.amountNano) : "—"}
              </b>
            </div>
          );
        })}
      </div>
    </td>
  );
}

interface RowProps {
  user: AdminUser;
  /** Действие, выполняющееся прямо сейчас для этой строки (кнопка disabled). */
  busyAction: string | null;
  onCredit: (user: AdminUser) => void;
  onAction: (user: AdminUser, action: UserAction) => void;
  onPartner: (user: AdminUser) => void;
}

// Строка таблицы мемоизирована: ререндер идёт на каждое изменение фильтров/действий.
const UserRow = memo(function UserRow({ user, busyAction, onCredit, onAction, onPartner }: RowProps): ReactElement {
  const { t } = useI18n();
  const pay = user.payments ?? {};
  const keys = user.api_keys ?? {};
  const methods = user.auth_methods ?? [];
  const statusKind = user.status === "disabled" ? "bad" : user.engine_live_status === "disabled" ? "warn" : "ok";
  return (
    <tr>
      <td className="left">
        <Dot kind={statusKind} /> <b>{user.email}</b>
        <div className="sub">
          {user.display_name || ""} · {methods.length ? methods.map((method) => <Pill key={method}>{method}</Pill>) : "—"}
          {user.email_verified ? <Pill kind="ok">email ✓</Pill> : <Pill kind="warn">email ✗</Pill>}
        </div>
      </td>
      <td className="left">
        {tierLabel(user)}
        <div className="sub">
          {user.multiplier_bp == null ? "множитель не получен" : `${user.multiplier_bp} bp · сохранённые условия`}
        </div>
      </td>
      <td>
        <b>{user.balance_usd == null ? "—" : money(user.balance_usd)}</b>
        {Number(user.reserved_usd) > 0 ? <div className="sub">резерв {money(user.reserved_usd)}</div> : null}
      </td>
      <td>
        {user.spent_usd == null ? "—" : money(user.spent_usd)}
        <div className="sub">30д {money(user.spent_30d_usd)}</div>
        {user.cumulative_topup_usd != null ? <div className="sub">пополнено всего {money(user.cumulative_topup_usd)}</div> : null}
      </td>
      <UserProviderSpend user={user} />
      <td>
        {pay.paid_count ? (
          <>
            {money(pay.paid_total_usd)}
            <div className="sub">
              {pay.paid_count} шт.{pay.last_paid_at ? ` · ${ago(pay.last_paid_at)}` : ""}
            </div>
          </>
        ) : (
          "—"
        )}
        {Number(pay.pending_checkouts) > 0 ? (
          <div className="sub">
            {count(Number(pay.pending_checkouts), "checkout ожидает оплату", "checkout ожидают оплату", "checkout ожидают оплату")}
          </div>
        ) : null}
      </td>
      <td>
        {Number(keys.active || 0)}/{Number(keys.total || 0)}
      </td>
      <td>{ago(user.last_seen_at)}</td>
      <td>{formatDate(user.created_at)}</td>
      <td>
        <div className="actions wrap">
          {user.engine_account_id && user.status === "active" ? (
            <button className="btn" disabled={busyAction === "credit"} onClick={() => onCredit(user)}>
              + подарок
            </button>
          ) : null}
          {user.engine_account_id && user.customer_type === "b2c" ? (
            <button className="btn" disabled={busyAction === "business"} onClick={() => onAction(user, "business")}>
              → B2B
            </button>
          ) : null}
          {user.id && user.email && user.status === "active" ? (
            <button type="button" className="btn ghost" disabled={busyAction === "partner"} onClick={() => onPartner(user)}>
              {t("Make Partner", "Сделать партнёром")}
            </button>
          ) : null}
          {user.engine_account_id ? (
            <button className="btn warn" disabled={busyAction === "bonus"} onClick={() => onAction(user, "bonus")}>
              − бонус
            </button>
          ) : null}
          <button className="btn ghost" disabled={busyAction === "sessions"} onClick={() => onAction(user, "sessions")}>
            сессии
          </button>
          {user.totp_enabled ? (
            <button className="btn warn" disabled={busyAction === "totp"} onClick={() => onAction(user, "totp")}>
              сброс 2FA
            </button>
          ) : null}
          <button
            className={"btn" + (user.status === "active" ? " bad" : "")}
            disabled={busyAction === "disable" || busyAction === "enable"}
            onClick={() => onAction(user, user.status === "active" ? "disable" : "enable")}
          >
            {user.status === "active" ? "отключить" : "включить"}
          </button>
        </div>
      </td>
    </tr>
  );
});

export default function UsersPage() {
  const [page, setPage] = useState<UserPageState>(INITIAL_USER_PAGE);
  // Черновик фильтров: «Найти» применяет всё; сортировка/направление — сразу при
  // выборе (в легаси sort.onchange/dir.onchange вызывают общий apply формы).
  const [draft, setDraft] = useState({ q: "", status: "", auth: "", sort: "created_at" as UserSortKey, dir: "desc" as "asc" | "desc" });
  const [busy, setBusy] = useState<{ userId: string; action: string } | null>(null);
  const [businessTarget, setBusinessTarget] = useState<BusinessConversionTarget | null>(null);
  const [partnerTarget, setPartnerTarget] = useState<PartnerOnboardingTarget | null>(null);

  const query = usersQuery(page);
  const { data: result, isLoading, refresh } = useResources<{
    page: AdminUsersPage;
    dashboard: CommerceDashboard;
    overview: UsersOverview;
  }>({
    page: `/admin/users?${query}`,
    dashboard: "/admin/dashboard",
    overview: compactOverviewUrl(),
  });
  const { openSpendStats, spendStatsModal } = useSpendStatsModal();

  // Сервер откатил offset за последнюю страницу — синхронизируем состояние,
  // чтобы пейджер и следующие запросы шли от фактического offset.
  const effectiveOffset = clampedOffset(page.offset, page.limit, result.page?.total ?? 0) ?? page.offset;

  useEffect(() => {
    if (result.page && effectiveOffset !== page.offset) {
      startTransition(() => setPage((current) => ({ ...current, offset: effectiveOffset })));
    }
  }, [effectiveOffset, page.offset, result.page]);

  // Применить весь черновик формы: легаси-apply читает все поля сразу, поэтому
  // смена сортировки подхватывает и ещё не отправленный текст поиска. `draft` в
  // замыкании свежий — контролируемые инпуты ререндерят страницу на каждый ввод.
  const applyDraft = (patch: Partial<typeof draft> = {}) => {
    const next = { ...draft, ...patch };
    setDraft(next);
    startTransition(() =>
      setPage((prev) => ({
        ...prev,
        offset: 0,
        q: next.q.trim(),
        status: next.status,
        auth: next.auth,
        sort: next.sort,
        dir: next.dir,
      })),
    );
  };

  const submitFilters = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    applyDraft();
  };

  const goTo = useCallback((offset: number) => {
    startTransition(() => setPage((prev) => ({ ...prev, offset })));
  }, []);

  // Gift credit is explicitly not external payment evidence and never becomes partner-commission
  // basis. Its idempotency key survives a dropped request in sessionStorage.
  const creditUser = useCallback(
    async (user: AdminUser) => {
      const values = await dialog({
        title: "Начислить подарочный кредит",
        message: `${user.email ?? "Пользователь"}\n\nЭто подарок платформы, не подтверждённая оплата. Он не считается выручкой и не создаёт партнёрскую комиссию.`,
        confirmLabel: "Начислить подарок",
        fields: [{ name: "amount", label: "Сумма USD — целое число 1–99999" }],
      });
      if (!values) return;
      const value = (values.amount || "").trim();
      if (!/^[1-9][0-9]{0,4}$/.test(value)) {
        toast("Сумма: целое число от 1 до 99999.", "bad");
        return;
      }
      const userId = String(user.id ?? "");
      setBusy({ userId, action: "credit" });
      const pendingKey = "admin-credit-pending:" + userId;
      const payloadSignature = value + "\n" + GIFT_CREDIT_REASON;
      let idempotencyKey = crypto.randomUUID();
      try {
        const pending = JSON.parse(sessionStorage.getItem(pendingKey) || "null") as
          | { signature?: string; idempotencyKey?: string }
          | null;
        if (pending?.signature === payloadSignature && pending.idempotencyKey) {
          idempotencyKey = pending.idempotencyKey;
        }
      } catch {
        // Битый JSON в sessionStorage — просто новый ключ.
      }
      sessionStorage.setItem(pendingKey, JSON.stringify({ signature: payloadSignature, idempotencyKey }));
      try {
        const result = await send<{ balance_usd?: number }>(`/admin/users/${userId}/balance-adjustments`, "POST", {
          amount_usd: value,
          reason: GIFT_CREDIT_REASON,
          idempotency_key: idempotencyKey,
        });
        sessionStorage.removeItem(pendingKey);
        toast("Готово. Новый баланс: " + money(result.balance_usd));
      } catch (cause) {
        toast(
          (cause instanceof Error ? cause.message : String(cause)) +
            " — idempotency key сохранён: повторите те же сумму и причину для безопасного retry.",
          "bad",
        );
      } finally {
        setBusy(null);
      }
    },
    [],
  );

  const userAction = useCallback(
    async (user: AdminUser, action: UserAction) => {
      if (action === "business") {
        const currentDiscount = user.multiplier_bp == null ? 50 : Math.round(100 - user.multiplier_bp / 100);
        setBusinessTarget({ user, initialDiscount: Math.min(95, Math.max(0, currentDiscount)) });
        return;
      }
      const values = await dialog({
        title: USER_ACTION_LABELS[action],
        message: user.email ?? undefined,
        confirmLabel: "Выполнить",
        fields: [],
        danger: action === "disable" || action === "bonus",
      });
      if (!values) return;
      const userId = String(user.id ?? "");
      setBusy({ userId, action });
      try {
        let result: UserActionResult = {};
        if (action === "disable" || action === "enable") {
          result = await send<UserActionResult>(`/admin/users/${userId}/status`, "PATCH", {
            status: action === "disable" ? "disabled" : "active",
            reason: PANEL_REASON,
          });
        }
        if (action === "sessions") {
          result = await send<UserActionResult>(`/admin/users/${userId}/sessions/revoke`, "POST", { reason: PANEL_REASON });
        }
        if (action === "totp") {
          result = await send<UserActionResult>(`/admin/users/${userId}/totp/reset`, "POST", { reason: PANEL_REASON });
        }
        if (action === "bonus") {
          result = await send<UserActionResult>(`/admin/users/${userId}/bonus/revoke`, "POST", { reason: PANEL_REASON });
        }
        toast(
          "Готово" +
            (result.sessions_revoked != null ? ` · сессий отозвано: ${result.sessions_revoked}` : "") +
            (result.customer_type === "b2b" ? ` · B2B, скидка ${result.discount_percent}%` : "") +
            (result.balance_usd != null
              ? ` · новый баланс: $${result.balance_usd}${result.idempotent_replay ? " (уже был отозван ранее)" : ""}`
              : ""),
        );
      } catch (cause) {
        toast(cause instanceof Error ? cause.message : String(cause), "bad");
      } finally {
        setBusy(null);
      }
    },
    [],
  );

  const convertToBusiness = useCallback(
    async (discountPercent: number) => {
      const target = businessTarget;
      if (!target) return;
      const userId = String(target.user.id ?? "");
      setBusy({ userId, action: "business" });
      try {
        const result = await send<UserActionResult>(`/admin/users/${userId}/convert-to-business`, "POST", {
          reason: PANEL_REASON,
          discountPercent,
        });
        setBusinessTarget(null);
        toast(`Готово · B2B, базовая скидка ${result.discount_percent ?? discountPercent}%`);
      } catch (cause) {
        toast(cause instanceof Error ? cause.message : String(cause), "bad");
      } finally {
        setBusy(null);
      }
    },
    [businessTarget],
  );

  if (isLoading && Object.values(result).every((value) => value === undefined)) {
    return (
      <>
        <PageHead title="Пользователи" sub="данные загружаются, навигация уже доступна" />
        <LoadingGrid />
      </>
    );
  }

  const usersPage = result.page ?? { users: [], total: 0, limit: page.limit, offset: effectiveOffset };
  const { dashboard, overview } = result;
  const users = usersPage.users ?? [];
  const total = usersPage.total ?? 0;
  const stats = dashboard?.users ?? {};
  const demand = overview?.demand ?? {};

  // «Видимые» суммы текущей страницы — легаси-поля USD, только отображение.
  const totalBalance = users.reduce((sum, user) => sum + Number(user.balance_usd || 0), 0);
  const totalSpent = users.reduce((sum, user) => sum + Number(user.spent_usd || 0), 0);

  return (
    <>
      <PageHead
        title="Пользователи"
        sub="серверный поиск, балансы, расходы по провайдерам, ключи и действия по клиентам"
        badge={<Pill kind="ok">{count(total, "клиент", "клиента", "клиентов")}</Pill>}
      />

      <CardGrid>
        <StatCard
          label="клиенты"
          value={total}
          hint={`${show(stats.registered_oauth)} OAuth-рег. · ${show(stats.registered_password)} обычных`}
        />
        <StatCard label="активны 7 дней" value={show(stats.active_7d)} hint={`${show(stats.disabled)} отключены`} />
        <StatCard
          label="баланс видимых"
          value={money(totalBalance)}
          hint={`${users.length} показанных${demand.balance_usd == null ? "" : " · платформа " + money(demand.balance_usd)}`}
        />
        <StatCard
          label="расход видимых"
          value={money(totalSpent)}
          hint={`текущая страница${demand.spent_usd == null ? "" : " · платформа " + money(demand.spent_usd)}`}
        />
      </CardGrid>

      <SectionHeader title="Все пользователи" />

      <form className="toolbar" onSubmit={submitFilters}>
        <label className="sr-only" htmlFor="search">
          Поиск пользователей
        </label>
        <input
          id="search"
          name="q"
          type="search"
          autoComplete="off"
          spellCheck={false}
          value={draft.q}
          onChange={(event) => setDraft((prev) => ({ ...prev, q: event.target.value }))}
          placeholder="email, имя или UUID…"
        />
        <label className="sr-only" htmlFor="status">
          Статус
        </label>
        <select
          id="status"
          value={draft.status}
          onChange={(event) => setDraft((prev) => ({ ...prev, status: event.target.value }))}
        >
          <option value="">все статусы</option>
          <option value="active">active</option>
          <option value="disabled">disabled</option>
        </select>
        <label className="sr-only" htmlFor="auth">
          Способ регистрации
        </label>
        <select id="auth" value={draft.auth} onChange={(event) => setDraft((prev) => ({ ...prev, auth: event.target.value }))}>
          <option value="">любая регистрация</option>
          <option value="password">password</option>
          <option value="google">Google</option>
          <option value="github">GitHub</option>
        </select>
        <label className="sr-only" htmlFor="sort">
          Сортировка
        </label>
        <select id="sort" value={draft.sort} onChange={(event) => applyDraft({ sort: event.target.value as UserSortKey })}>
          {USER_SORTS.map(([value, label]) => (
            <option key={value} value={value}>
              {label}
            </option>
          ))}
        </select>
        <label className="sr-only" htmlFor="dir">
          Направление
        </label>
        <select id="dir" value={draft.dir} onChange={(event) => applyDraft({ dir: event.target.value as "asc" | "desc" })}>
          <option value="desc">по убыванию</option>
          <option value="asc">по возрастанию</option>
        </select>
        <button className="btn" type="submit">
          Найти
        </button>
        <button
          className="btn ghost"
          type="button"
          title="Выгрузить текущую страницу в CSV"
          onClick={() => downloadCsv(`users-${csvDate()}.csv`, USERS_CSV_HEADER, buildUsersCsvRows(users))}
        >
          CSV
        </button>
      </form>

      <TableCard>
        <table className="users-table">
          <thead>
            <tr>
              <th className="left">пользователь</th>
              <th className="left">тариф</th>
              <th>баланс</th>
              <th>
                <button
                  type="button"
                  className="table-action"
                  onClick={openSpendStats}
                  title="Разбивка: сутки / 7 дней / 30 дней"
                >
                  потрачено
                </button>
              </th>
              <th>
                <span className="user-provider-heading">провайдеры <small>30д</small></span>
              </th>
              <th>пополнения</th>
              <th>ключи</th>
              <th>был(а)</th>
              <th>регистрация</th>
              <th>действия</th>
            </tr>
          </thead>
          <tbody>
            {users.length ? (
              users.map((user, index) => (
                <UserRow
                  key={user.id ?? user.email ?? index}
                  user={user}
                  busyAction={busy && busy.userId === String(user.id ?? "") ? busy.action : null}
                  onCredit={creditUser}
                  onAction={userAction}
                  onPartner={(target) => {
                    if (target.id && target.email) setPartnerTarget({ id: target.id, email: target.email });
                  }}
                />
              ))
            ) : (
              <EmptyRow columns={10} />
            )}
          </tbody>
        </table>
      </TableCard>

      <div className="pager">
        <span>
          {total ? effectiveOffset + 1 : 0}–{Math.min(effectiveOffset + page.limit, total)} из {total}
        </span>
        <button
          type="button"
          className="btn ghost"
          disabled={effectiveOffset <= 0}
          onClick={() => goTo(Math.max(0, effectiveOffset - page.limit))}
        >
          Назад
        </button>
        <button
          type="button"
          className="btn ghost"
          disabled={effectiveOffset + page.limit >= total}
          onClick={() => goTo(effectiveOffset + page.limit)}
        >
          Дальше
        </button>
      </div>

      <footer>Отключение синхронно блокирует engine-аккаунт и отзывает все сессии. Каждое действие аудируется.</footer>

      {spendStatsModal}
      <BusinessConversionDialog
        target={businessTarget}
        submitting={busy?.action === "business"}
        onClose={() => setBusinessTarget(null)}
        onConfirm={(discountPercent) => void convertToBusiness(discountPercent)}
      />
      <PartnerOnboardingDialog
        target={partnerTarget}
        onClose={() => setPartnerTarget(null)}
        onCreated={refresh}
      />
    </>
  );
}
