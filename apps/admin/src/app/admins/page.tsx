"use client";

// Админы — порт 1:1 admins()/bindAdmins()/adminAction() из
// crates/server/src/admin-panel.js (строки 348-393): центральное управление
// администраторами внутренних доменов — создание, смена пароля, domain grants,
// отключение/включение. Автоматические обновления приходят из commerce SSE.
import { Fragment, memo, useCallback, useMemo, useState, type FormEvent } from "react";
import { send } from "@/lib/api";
import { useResources } from "@/lib/resources";
import { formatDate } from "@/lib/format";
import { toast } from "@/lib/toast";
import { dialog } from "@/lib/dialog";
import { Banner, EmptyRow, LoadingGrid, PageHead, Pill, SectionHeader, TableCard } from "@/components/ui";
import { isLastActiveAdmin, parseDomainsInput } from "./lib";

// reason для всех мутаций — дословно PANEL_REASON из admin-panel.js.
const PANEL_REASON = "ручное действие из админ-панели";

interface AdminAccount {
  id?: string;
  username?: string;
  status?: string;
  domains?: string[];
  password_changed_at?: string;
  created_at?: string;
}

interface AdminAccountsResponse {
  accounts?: AdminAccount[];
  current_account_id?: string;
}

interface AdminDomain {
  domain?: string;
  label?: string;
}

interface AdminDomainsResponse {
  domains?: AdminDomain[];
  external_domains?: { domain?: string; account_system?: string }[];
}

interface MutationResult {
  changed_self?: boolean;
}

// Статичная разметка — вынесена из компонента страницы.
const TABLE_HEAD = (
  <thead>
    <tr>
      <th className="left">администратор</th>
      <th className="left">домены</th>
      <th>статус</th>
      <th>пароль изменён</th>
      <th>создан</th>
      <th>действия</th>
    </tr>
  </thead>
);

const FOOTER = (
  <footer>
    Пароли хранятся только как Argon2id-хеши. Нельзя отключить или лишить main-admin доступа последнего активного
    администратора.
  </footer>
);

interface AdminRowProps {
  account: AdminAccount;
  self: boolean;
  busy: boolean;
  onPassword: (account: AdminAccount) => void;
  onDomains: (account: AdminAccount) => void;
  onStatus: (account: AdminAccount) => void;
}

const AdminRow = memo(function AdminRow({ account, self, busy, onPassword, onDomains, onStatus }: AdminRowProps) {
  const active = account.status === "active";
  return (
    <tr>
      <td className="left">
        <b>{account.username}</b>
        {self ? (
          <>
            {" "}
            <Pill kind="info">вы</Pill>
          </>
        ) : null}
        <div className="sub mono">{account.id}</div>
      </td>
      <td className="left domain-list">
        {(account.domains ?? []).map((domain, index) => (
          <Fragment key={domain}>
            {index > 0 ? " " : null}
            <Pill kind={domain === "admin.apitoken.sale" ? "info" : ""}>{domain}</Pill>
          </Fragment>
        ))}
      </td>
      <td>
        <Pill kind={active ? "ok" : "bad"}>{account.status}</Pill>
      </td>
      <td>{formatDate(account.password_changed_at, true)}</td>
      <td>{formatDate(account.created_at, true)}</td>
      <td>
        <div className="actions">
          <button className="btn" disabled={busy} onClick={() => onPassword(account)}>
            пароль
          </button>
          <button className="btn" disabled={busy} onClick={() => onDomains(account)}>
            домены
          </button>
          <button className={"btn" + (active ? " bad" : "")} disabled={busy} onClick={() => onStatus(account)}>
            {active ? "отключить" : "включить"}
          </button>
        </div>
      </td>
    </tr>
  );
});

export default function AdminsPage() {
  const [domainFilter, setDomainFilter] = useState("");
  const [busy, setBusy] = useState(false);
  const [creating, setCreating] = useState(false);
  // Фильтр по домену — часть URL: смена фильтра получает отдельный кэшируемый
  // снапшот и не перезапрашивает справочник доменов.
  const dataPath = domainFilter
    ? `/admin/admin-accounts?domain=${encodeURIComponent(domainFilter)}`
    : "/admin/admin-accounts";
  const { data: result, isLoading } = useResources<{
    data: AdminAccountsResponse;
    directory: AdminDomainsResponse;
  }>({
    data: dataPath,
    directory: "/admin/admin-accounts/domains",
  });

  const accounts = useMemo(() => result?.data?.accounts ?? [], [result]);
  const domains = useMemo(() => result?.directory?.domains ?? [], [result]);
  const allowed = useMemo(() => domains.map((item) => String(item.domain ?? "")), [domains]);

  // Общий хвост всех трёх PATCH-мутаций (adminAction): PATCH → тост → обновление.
  // Отдельный тост «список не обновился» из легаси не нужен — сбой источника
  // показывает ErrorCenter общего URL-store.
  const runMutation = useCallback(
    async (path: string, body: unknown) => {
      setBusy(true);
      try {
        const mutation = await send<MutationResult>(path, "PATCH", body);
        toast(
          "Изменение сохранено." +
            (mutation.changed_self ? " Введите новый пароль при следующем запросе." : ""),
        );
      } catch (error) {
        toast("Изменение не сохранено: " + (error instanceof Error ? error.message : String(error)), "bad");
      } finally {
        setBusy(false);
      }
    },
    [],
  );

  const handlePassword = useCallback(
    async (account: AdminAccount) => {
      const self = Boolean(account.id) && account.id === result?.data?.current_account_id;
      const values = await dialog({
        title: "Сменить пароль " + account.username,
        confirmLabel: "Сменить",
        message: self ? "Это ваш аккаунт: браузер запросит новые credentials." : "",
        fields: [
          { name: "first", label: "Новый пароль (минимум 8 символов)", type: "password" },
          { name: "second", label: "Повторите пароль", type: "password" },
        ],
      });
      if (!values) return;
      if ((values.first || "").length < 8) return toast("Пароль слишком короткий.", "bad");
      if (values.second !== values.first) return toast("Пароли не совпадают.", "bad");
      await runMutation(`/admin/admin-accounts/${account.id}/password`, {
        password: values.first,
        reason: PANEL_REASON,
      });
    },
    [result, runMutation],
  );

  const handleDomains = useCallback(
    async (account: AdminAccount) => {
      const values = await dialog({
        title: "Домены для " + account.username,
        message: "Через запятую. Доступные: " + allowed.join(", "),
        confirmLabel: "Сохранить",
        fields: [{ name: "value", label: "Домены", value: (account.domains ?? []).join(",") }],
      });
      if (!values) return;
      const selected = parseDomainsInput(values.value || "", allowed);
      if (!selected) return toast("Укажите один или несколько доменов ровно как в списке.", "bad");
      await runMutation(`/admin/admin-accounts/${account.id}/domains`, {
        domains: selected,
        reason: PANEL_REASON,
      });
    },
    [allowed, runMutation],
  );

  const handleStatus = useCallback(
    async (account: AdminAccount) => {
      const next = account.status === "active" ? "disabled" : "active";
      // Клиентская страховка «последний активный администратор» (сервер тоже
      // отклоняет). При активном фильтре по домену список неполный — там решает сервер.
      if (next === "disabled" && !domainFilter && isLastActiveAdmin(accounts, account.id ?? "")) {
        return toast("Нельзя отключить последнего активного администратора.", "bad");
      }
      const values = await dialog({
        title: (next === "disabled" ? "Отключить " : "Включить ") + account.username,
        confirmLabel: "Выполнить",
        danger: next === "disabled",
      });
      if (!values) return;
      await runMutation(`/admin/admin-accounts/${account.id}/status`, { status: next, reason: PANEL_REASON });
    },
    [accounts, domainFilter, runMutation],
  );

  // Создание администратора (форма «Новый администратор»): native-валидация
  // required/pattern/minlength, затем подтверждение через dialog, как в легаси.
  const submitCreate = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const form = event.currentTarget;
    const data = new FormData(form);
    const selected = data.getAll("domains").map(String);
    if (!selected.length) return toast("Выберите хотя бы один домен.", "bad");
    const values = await dialog({
      title: "Создать администратора",
      message: String(data.get("username") || ""),
      confirmLabel: "Создать",
    });
    if (!values) return;
    setCreating(true);
    try {
      await send("/admin/admin-accounts", "POST", {
        username: String(data.get("username") ?? ""),
        password: String(data.get("password") ?? ""),
        domains: selected,
        reason: PANEL_REASON,
      });
      form.reset();
      toast("Администратор создан.");
    } catch (error) {
      toast(error instanceof Error ? error.message : String(error), "bad");
    } finally {
      setCreating(false);
    }
  };

  if (isLoading && Object.values(result).every((value) => value === undefined)) {
    return (
      <>
        <PageHead title="Админы" sub="данные загружаются, навигация уже доступна" />
        <LoadingGrid />
      </>
    );
  }

  const currentId = result.data?.current_account_id;
  const external = (result.directory?.external_domains ?? [])
    .map((item) => `${item.domain} использует ${item.account_system}`)
    .join(" · ");

  return (
    <>
      <PageHead
        title="Админы"
        sub="identity и domain grants для управляемых доменов"
        badge={<Pill kind="ok">{accounts.filter((account) => account.status === "active").length} active</Pill>}
      />

      <Banner kind="ok" title="Центральное управление администраторами">
        Один логин можно назначить на один или несколько внутренних доменов. {external}.
      </Banner>

      <SectionHeader title="Новый администратор" />
      <form className="form-card form admin-form" onSubmit={submitCreate}>
        <div className="field">
          <label>Логин</label>
          <input
            name="username"
            required
            maxLength={80}
            pattern="[A-Za-z0-9._@-]+"
            autoComplete="off"
            placeholder="new.admin"
          />
        </div>
        <div className="field">
          <label>Пароль (минимум 8)</label>
          <input name="password" type="password" required minLength={8} maxLength={200} autoComplete="new-password" />
        </div>
        <div className="field">
          <label>Доступ к доменам</label>
          <div className="checks">
            {domains.map((item, index) => (
              <label className="check" key={item.domain}>
                <input type="checkbox" name="domains" value={item.domain} defaultChecked={index === 0} />
                {item.label}
              </label>
            ))}
          </div>
        </div>
        <button className="btn" type="submit" disabled={creating}>
          создать
        </button>
      </form>

      <SectionHeader title="Администраторы" sub={`точный фильтр по домену · найдено ${accounts.length}`} />
      <div className="toolbar">
        <select
          aria-label="Фильтр по домену"
          value={domainFilter}
          onChange={(event) => setDomainFilter(event.target.value)}
        >
          <option value="">все управляемые домены</option>
          {domains.map((item) => (
            <option key={item.domain} value={item.domain}>
              {item.domain}
            </option>
          ))}
        </select>
      </div>

      <TableCard>
        <table>
          {TABLE_HEAD}
          <tbody>
            {accounts.length ? (
              accounts.map((account) => (
                <AdminRow
                  key={account.id}
                  account={account}
                  self={Boolean(account.id) && account.id === currentId}
                  busy={busy}
                  onPassword={handlePassword}
                  onDomains={handleDomains}
                  onStatus={handleStatus}
                />
              ))
            ) : (
              <EmptyRow columns={6} />
            )}
          </tbody>
        </table>
      </TableCard>

      {FOOTER}
    </>
  );
}
