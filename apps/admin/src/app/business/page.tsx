"use client";

// B2B — порт 1:1 функций business()/bindBusiness() из crates/server/src/admin-panel.js
// (строки 892-938): B2B-клиенты с индивидуальными скидками и инвайты с идемпотентным
// созданием, переотправкой письма, отзывом и копированием ссылки.
// Автоопроса у вкладки нет (как в легаси) — только фокус/кнопка ↻.
import { memo, useCallback, useState, type FormEvent } from "react";
import { api, send } from "@/lib/api";
import { usePoll } from "@/lib/usePoll";
import { count, formatDate, money } from "@/lib/format";
import { dialog } from "@/lib/dialog";
import { toast } from "@/lib/toast";
import { EmptyRow, LoadingGrid, Modal, PageHead, Pill, SectionHeader, TableCard } from "@/components/ui";
import {
  canonicalizePricingRules,
  ManagedPolicyEditor,
  PolicyRuleEditor,
  pricingRulesSignature,
  type ManagedPolicyView,
  type PricingCatalogView,
  type PricingRule,
} from "./policy-editor";
import {
  copyText,
  deliveryPill,
  inviteState,
  isInviteActive,
  parseBoundedInteger,
  reuseIdempotencyKey,
  INVITE_PENDING_KEY,
  PANEL_REASON,
  RESEND_PENDING_PREFIX,
  type BusinessInvite,
  type BusinessInvitesPage,
  type BusinessUser,
  type BusinessUsersPage,
} from "./utils";

const CLIENT_LIMIT = 50;
const INVITES_LIMIT = 100;

type PolicyDialogState = {
  key: string;
  title: string;
  readPath: string;
  savePath: string;
  nestedMutation: boolean;
  policy: ManagedPolicyView | null;
  rules: PricingRule[];
  loading: boolean;
  saving: boolean;
  error: string | null;
};

interface BusinessData {
  invites: BusinessInvitesPage | null;
  users: BusinessUsersPage | null;
  catalog: PricingCatalogView | null;
  /** Фактический offset загруженной страницы (после отсечения за конец списка). */
  offset: number;
}

// Оба источника параллельно; падение любого → null, таблица показывает «данных нет».
// Как в легаси: offset за концом списка (клиентов стало меньше) — повторный запрос
// последней страницы тем же fetcher'ом, без setState в эффектах.
async function loadBusiness(offset: number): Promise<BusinessData> {
  const [invites, users, catalog] = await Promise.all([
    api<BusinessInvitesPage>(`/admin/business-invites?limit=${INVITES_LIMIT}`).catch(() => null),
    api<BusinessUsersPage>(`/admin/users?limit=${CLIENT_LIMIT}&offset=${offset}&customer_type=b2b`).catch(() => null),
    api<PricingCatalogView>("/admin/pricing-catalog").catch(() => null),
  ]);
  const total = users?.total ?? users?.users?.length ?? 0;
  if (users && total > 0 && offset >= total) {
    const clamped = Math.max(0, Math.floor((total - 1) / CLIENT_LIMIT) * CLIENT_LIMIT);
    const refetched = await api<BusinessUsersPage>(
      `/admin/users?limit=${CLIENT_LIMIT}&offset=${clamped}&customer_type=b2b`,
    ).catch(() => null);
    return { invites, users: refetched, catalog, offset: clamped };
  }
  return { invites, users, catalog, offset };
}

const errorText = (error: unknown): string => (error instanceof Error ? error.message : String(error));

function forgetPending(storageKey: string): void {
  try {
    sessionStorage.removeItem(storageKey);
  } catch {
    // Хранилище недоступно — ключ просто останется до перезаписи.
  }
}

// Форма создания инвайта — разметка дословно из легаси (form-card form).
const InviteForm = memo(function InviteForm(props: {
  submitting: boolean;
  catalog: PricingCatalogView | null;
  rules: PricingRule[];
  onRulesChange: (rules: PricingRule[]) => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
}) {
  return (
    <form className="form-card form" onSubmit={props.onSubmit}>
      <div className="field">
        <label>Email (необязательно)</label>
        <input name="email" type="email" placeholder="client@company.com" autoComplete="off" />
        <div className="sub">Есть email — письмо уйдёт автоматически. Нет email — ссылка скопируется.</div>
      </div>
      <div className="field">
        <label>Срок, дней</label>
        <input name="days" type="number" min={1} max={30} defaultValue={7} required />
      </div>
      <button className="btn" type="submit" disabled={props.submitting || !props.catalog || props.rules.length === 0}>
        создать инвайт
      </button>
      <div style={{ gridColumn: "1 / -1" }}>
        {props.catalog ? (
          <PolicyRuleEditor
            catalog={props.catalog}
            rules={props.rules}
            onChange={props.onRulesChange}
            segment="b2b"
            disabled={props.submitting}
          />
        ) : (
          <div className="policy-rule-count bad">Pricing foundation ещё не материализован: policy-based инвайты недоступны.</div>
        )}
      </div>
    </form>
  );
});

const CLIENTS_HEAD = (
  <thead>
    <tr>
      <th className="left">клиент</th>
      <th>политика</th>
      <th>баланс</th>
      <th>engine</th>
      <th>синхронизация цены</th>
      <th />
    </tr>
  </thead>
);

const INVITES_HEAD = (
  <thead>
    <tr>
      <th className="left">получатель</th>
      <th>политика</th>
      <th>статус</th>
      <th>доставка</th>
      <th>истекает</th>
      <th>действия</th>
    </tr>
  </thead>
);

const ClientRow = memo(function ClientRow(props: {
  user: BusinessUser;
  busy: boolean;
  onPricing: (user: BusinessUser) => void;
}) {
  const { user } = props;
  const syncStatus = user.pricing_sync_status ?? "—";
  return (
    <tr>
      <td className="left">
        <b>{user.email}</b>
        <div className="sub mono">{user.id}</div>
      </td>
      <td>
        <b>provider / model</b>
        <div className="sub">открыть точную версию</div>
      </td>
      <td>{money(user.balance_usd)}</td>
      <td>
        <Pill kind={user.engine_account_status === "active" ? "ok" : "warn"}>
          {user.engine_account_status ?? "—"}
        </Pill>
      </td>
      <td>
        <Pill kind={syncStatus === "confirmed" ? "ok" : syncStatus === "failed" ? "bad" : "warn"}>{syncStatus}</Pill>
        {user.pricing_sync_error ? <div className="sub">{user.pricing_sync_error}</div> : null}
      </td>
      <td>
        <button className="btn" disabled={props.busy} onClick={() => props.onPricing(user)}>
          открыть политику
        </button>
      </td>
    </tr>
  );
});

const InviteRow = memo(function InviteRow(props: {
  invite: BusinessInvite;
  busyCopy: boolean;
  busyResend: boolean;
  busyRevoke: boolean;
  onCopy: (id: string) => void;
  onResend: (id: string) => void;
  onRevoke: (id: string) => void;
  onPricing: (invite: BusinessInvite) => void;
}) {
  const { invite } = props;
  const state = inviteState(invite);
  const delivery = deliveryPill(invite);
  const active = isInviteActive(invite);
  return (
    <tr>
      <td className="left">
        <b>{invite.email || "Без привязки к email"}</b>
        <div className="sub mono">{invite.id}</div>
      </td>
      <td>{invite.policy_version == null ? (
        invite.discount_percent == null ? "legacy" : `legacy ${invite.discount_percent}%`
      ) : (
        <><b>v{invite.policy_version}</b><div className="sub">{invite.policy_rule_count ?? 0} правил</div></>
      )}</td>
      <td>
        <Pill kind={state.kind}>{state.label}</Pill>
      </td>
      <td>
        <Pill kind={delivery.kind}>{delivery.label}</Pill>
        {invite.delivery_error ? <div className="sub">{invite.delivery_error}</div> : null}
      </td>
      <td>{formatDate(invite.expires_at, true)}</td>
      <td>
        {active ? (
          <div className="actions wrap">
            <button className="btn" disabled={props.busyCopy} onClick={() => props.onCopy(invite.id)}>
              копировать
            </button>
            {invite.email ? (
              <button className="btn" disabled={props.busyResend} onClick={() => props.onResend(invite.id)}>
                отправить заново
              </button>
            ) : null}
            {invite.policy_version != null ? (
              <button className="btn" onClick={() => props.onPricing(invite)}>политика</button>
            ) : null}
            <button className="btn bad" disabled={props.busyRevoke} onClick={() => props.onRevoke(invite.id)}>
              отозвать
            </button>
          </div>
        ) : null}
      </td>
    </tr>
  );
});

function Pager(props: { offset: number; limit: number; total: number; onPage: (offset: number) => void }) {
  const { offset, limit, total } = props;
  return (
    <div className="pager">
      <span>
        {total ? offset + 1 : 0}–{Math.min(offset + limit, total)} из {total}
      </span>
      <button type="button" className="btn ghost" disabled={offset <= 0} onClick={() => props.onPage(Math.max(0, offset - limit))}>
        Назад
      </button>
      <button
        type="button"
        className="btn ghost"
        disabled={offset + limit >= total}
        onClick={() => props.onPage(offset + limit)}
      >
        Дальше
      </button>
    </div>
  );
}

function PolicyDialog(props: {
  state: PolicyDialogState | null;
  catalog: PricingCatalogView | null;
  onClose: () => void;
  onRulesChange: (rules: PricingRule[]) => void;
  onSave: () => void;
}) {
  const state = props.state;
  return (
    <Modal open={state !== null} title={state?.title ?? "Политика"} wide onClose={props.onClose}>
      {state?.loading ? <LoadingGrid count={3} /> : null}
      {state?.error ? <div className="policy-rule-count bad">{state.error}</div> : null}
      {state?.policy && props.catalog ? (
        <ManagedPolicyEditor
          catalog={props.catalog}
          policy={state.policy}
          rules={state.rules}
          onRulesChange={props.onRulesChange}
          segment="b2b"
          disabled={state.saving}
        />
      ) : null}
      {state && !state.loading && state.policy && !props.catalog ? (
        <div className="policy-rule-count bad">Pricing catalog недоступен: редактирование заблокировано.</div>
      ) : null}
      <div className="dlg-actions">
        <button type="button" className="btn ghost" onClick={props.onClose}>Закрыть</button>
        <button
          type="button"
          className="btn"
          disabled={!state?.policy || !props.catalog || state.rules.length === 0 || state.saving}
          onClick={props.onSave}
        >
          {state?.saving ? "сохраняем…" : "сохранить replacement policy"}
        </button>
      </div>
    </Modal>
  );
}

export default function BusinessPage() {
  const [requestedOffset, setOffset] = useState(0);
  const [busyIds, setBusyIds] = useState<ReadonlySet<string>>(() => new Set<string>());
  const [inviteRules, setInviteRules] = useState<PricingRule[]>([]);
  const [policyDialog, setPolicyDialog] = useState<PolicyDialogState | null>(null);
  // Ключ включает offset: у каждой страницы свой poller со своим кэшем (stale-while-revalidate).
  const { data: result, refresh } = usePoll(`business-${requestedOffset}`, () => loadBusiness(requestedOffset));

  const addBusy = useCallback((id: string) => {
    setBusyIds((prev) => {
      if (prev.has(id)) return prev;
      const next = new Set(prev);
      next.add(id);
      return next;
    });
  }, []);
  const dropBusy = useCallback((id: string) => {
    setBusyIds((prev) => {
      if (!prev.has(id)) return prev;
      const next = new Set(prev);
      next.delete(id);
      return next;
    });
  }, []);

  const clients = result?.users?.users ?? [];
  const clientTotal = result?.users?.total ?? clients.length;
  // Отображаемый offset — из данных (после отсечения в loadBusiness), не из запроса.
  const offset = result?.offset ?? requestedOffset;

  const submitInvite = useCallback(
    async (event: FormEvent<HTMLFormElement>) => {
      event.preventDefault();
      const form = event.currentTarget;
      const data = new FormData(form);
      const email = String(data.get("email") ?? "").trim();
      const expiresInDays = Number(data.get("days"));
      const reason = PANEL_REASON;
      const rules = canonicalizePricingRules(inviteRules);
      // Подпись параметров + сохранённый ключ: безопасный повтор после сетевой ошибки.
      const signature = [email, expiresInDays, reason, pricingRulesSignature(rules)].join("\n");
      const idempotencyKey = reuseIdempotencyKey(INVITE_PENDING_KEY, signature);
      const payload: Record<string, unknown> = { policy: { rules }, expiresInDays, reason, idempotencyKey };
      if (email) payload.email = email;
      addBusy("submit");
      try {
        const result = await send<{ inviteUrl?: string }>("/admin/business-invites", "POST", payload);
        forgetPending(INVITE_PENDING_KEY);
        if (email) {
          toast(`Инвайт создан: письмо поставлено в очередь для ${email}`);
        } else {
          if (!result.inviteUrl) throw new Error("Ссылка недоступна");
          await copyText(result.inviteUrl);
          toast("Инвайт создан, ссылка скопирована.");
        }
        // Легаси перерисовывал shell — форма возвращалась к значениям по умолчанию.
        form.reset();
        setInviteRules([]);
        refresh();
      } catch (error) {
        toast(`${errorText(error)} — безопасный ключ повтора сохранён.`, "bad");
      } finally {
        dropBusy("submit");
      }
    },
    [addBusy, dropBusy, inviteRules, refresh],
  );

  const copyInviteLink = useCallback(
    async (id: string) => {
      addBusy(`copy:${id}`);
      try {
        const result = await api<{ inviteUrl?: string }>(`/admin/business-invites/${id}/link`);
        if (!result.inviteUrl) throw new Error("Ссылка недоступна");
        await copyText(result.inviteUrl);
        toast("Ссылка скопирована.");
      } catch (error) {
        toast(errorText(error), "bad");
      } finally {
        dropBusy(`copy:${id}`);
      }
    },
    [addBusy, dropBusy],
  );

  const revokeInvite = useCallback(
    async (id: string) => {
      const values = await dialog({ title: "Отозвать B2B-инвайт", confirmLabel: "Отозвать", danger: true });
      if (!values) return;
      addBusy(`revoke:${id}`);
      try {
        await send(`/admin/business-invites/${id}/revoke`, "POST", { reason: PANEL_REASON });
        toast("Инвайт отозван.");
        refresh();
      } catch (error) {
        toast(errorText(error), "bad");
      } finally {
        dropBusy(`revoke:${id}`);
      }
    },
    [addBusy, dropBusy, refresh],
  );

  const resendInvite = useCallback(
    async (id: string) => {
      const values = await dialog({
        title: "Заменить ссылку и отправить новое письмо",
        confirmLabel: "Отправить",
        fields: [{ name: "days", label: "Новый срок, дней (1–30)", value: "7" }],
      });
      if (!values) return;
      const days = parseBoundedInteger(values.days ?? "", 1, 30);
      if (days == null) {
        toast("Срок: целое число от 1 до 30.", "bad");
        return;
      }
      const pendingKey = RESEND_PENDING_PREFIX + id;
      const idempotencyKey = reuseIdempotencyKey(pendingKey, String(days));
      addBusy(`resend:${id}`);
      try {
        await send(`/admin/business-invites/${id}/resend`, "POST", {
          reason: PANEL_REASON,
          expiresInDays: days,
          idempotencyKey,
        });
        forgetPending(pendingKey);
        toast("Старая ссылка отозвана, новое письмо поставлено в очередь.");
        refresh();
      } catch (error) {
        toast(`${errorText(error)} — безопасный ключ повтора сохранён.`, "bad");
      } finally {
        dropBusy(`resend:${id}`);
      }
    },
    [addBusy, dropBusy, refresh],
  );

  const openPolicy = useCallback(async (target: {
    key: string;
    title: string;
    readPath: string;
    savePath: string;
    nestedMutation: boolean;
  }) => {
    setPolicyDialog({ ...target, policy: null, rules: [], loading: true, saving: false, error: null });
    try {
      const policy = await api<ManagedPolicyView>(target.readPath);
      setPolicyDialog((current) => current?.key === target.key
        ? { ...current, policy, rules: canonicalizePricingRules(policy.rules), loading: false }
        : current);
    } catch (error) {
      setPolicyDialog((current) => current?.key === target.key
        ? { ...current, loading: false, error: errorText(error) }
        : current);
    }
  }, []);

  const openClientPolicy = useCallback((user: BusinessUser) => {
    void openPolicy({
      key: `client:${user.id}`,
      title: `Pricing policy · ${user.email}`,
      readPath: `/admin/business-users/${user.id}/pricing-policy`,
      savePath: `/admin/business-users/${user.id}/pricing`,
      nestedMutation: true,
    });
  }, [openPolicy]);

  const openInvitePolicy = useCallback((invite: BusinessInvite) => {
    void openPolicy({
      key: `invite:${invite.id}`,
      title: `Pricing policy · ${invite.email || invite.id}`,
      readPath: `/admin/business-invites/${invite.id}/pricing-policy`,
      savePath: `/admin/business-invites/${invite.id}/pricing-policy`,
      nestedMutation: false,
    });
  }, [openPolicy]);

  const savePolicy = useCallback(async () => {
    const state = policyDialog;
    if (!state?.policy || state.rules.length === 0) return;
    const rules = canonicalizePricingRules(state.rules);
    const mutation = { expectedVersion: state.policy.currentVersion, rules };
    const body = state.nestedMutation
      ? { policy: mutation, reason: PANEL_REASON }
      : { ...mutation, reason: PANEL_REASON };
    setPolicyDialog((current) => current?.key === state.key ? { ...current, saving: true, error: null } : current);
    try {
      const response = state.nestedMutation
        ? await send<{ policy: ManagedPolicyView }>(state.savePath, "PATCH", body)
        : await send<ManagedPolicyView>(state.savePath, "PATCH", body);
      const policy = "policy" in response ? response.policy : response;
      setPolicyDialog((current) => current?.key === state.key
        ? { ...current, policy, rules: canonicalizePricingRules(policy.rules), saving: false }
        : current);
      toast(`Policy v${policy.currentVersion} сохранена; ожидаем exact ACK.`);
      refresh();
    } catch (error) {
      setPolicyDialog((current) => current?.key === state.key
        ? { ...current, saving: false, error: errorText(error) }
        : current);
    }
  }, [policyDialog, refresh]);

  if (!result) {
    return (
      <>
        <PageHead title="B2B" sub="данные загружаются, навигация уже доступна" />
        <LoadingGrid />
      </>
    );
  }

  const invites = result.invites?.invites ?? [];

  return (
    <>
      <PageHead
        title="B2B"
        sub="полные provider/model policies, invitation snapshots и exact engine ACK"
        badge={<Pill kind="ok">{count(clientTotal, "клиент", "клиента", "клиентов")}</Pill>}
      />

      <SectionHeader title="Новый B2B-инвайт" />
      <InviteForm
        submitting={busyIds.has("submit")}
        catalog={result.catalog}
        rules={inviteRules}
        onRulesChange={setInviteRules}
        onSubmit={submitInvite}
      />

      <SectionHeader title="B2B-клиенты" sub={String(clientTotal)} />
      <TableCard>
        <table>
          {CLIENTS_HEAD}
          <tbody>
            {clients.length ? (
              clients.map((user) => (
                <ClientRow
                  key={user.id}
                  user={user}
                  busy={busyIds.has(`pricing:${user.id}`)}
                  onPricing={openClientPolicy}
                />
              ))
            ) : (
              <EmptyRow columns={6} />
            )}
          </tbody>
        </table>
      </TableCard>

      <SectionHeader title="Последние инвайты" sub={String(invites.length)} />
      <TableCard>
        <table>
          {INVITES_HEAD}
          <tbody>
            {invites.length ? (
              invites.map((invite) => (
                <InviteRow
                  key={invite.id}
                  invite={invite}
                  busyCopy={busyIds.has(`copy:${invite.id}`)}
                  busyResend={busyIds.has(`resend:${invite.id}`)}
                  busyRevoke={busyIds.has(`revoke:${invite.id}`)}
                  onCopy={copyInviteLink}
                  onResend={resendInvite}
                  onRevoke={revokeInvite}
                  onPricing={openInvitePolicy}
                />
              ))
            ) : (
              <EmptyRow columns={6} />
            )}
          </tbody>
        </table>
      </TableCard>

      <Pager offset={offset} limit={CLIENT_LIMIT} total={clientTotal} onPage={setOffset} />
      <PolicyDialog
        state={policyDialog}
        catalog={result.catalog}
        onClose={() => setPolicyDialog(null)}
        onRulesChange={(rules) => setPolicyDialog((current) => current ? { ...current, rules } : current)}
        onSave={() => void savePolicy()}
      />
    </>
  );
}
