"use client";

// B2B — порт 1:1 функций business()/bindBusiness() из crates/server/src/admin-panel.js
// (строки 892-938): B2B-клиенты с индивидуальными скидками и инвайты с идемпотентным
// созданием, переотправкой письма, отзывом и копированием ссылки.
// Автоматические обновления приходят из commerce SSE, polling отсутствует.
import { memo, startTransition, useCallback, useEffect, useState, type FormEvent, type ReactElement } from "react";
import { api, send } from "@/lib/api";
import { useResources } from "@/lib/resources";
import { count, formatDate, money } from "@/lib/format";
import { dialog } from "@/lib/dialog";
import { toast } from "@/lib/toast";
import { EmptyRow, LoadingGrid, PageHead, Pill, TableCard } from "@/components/ui";
import { DiscountDialog, type DiscountDialogTarget } from "./discount-dialog";
import {
  copyText,
  deliveryPill,
  discountFromMultiplierBp,
  engineStatusPresentation,
  inviteState,
  isInviteActive,
  parseBoundedInteger,
  pricingSyncPresentation,
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

const errorText = (error: unknown): string => (error instanceof Error ? error.message : String(error));

function forgetPending(storageKey: string): void {
  try {
    sessionStorage.removeItem(storageKey);
  } catch {
    // Хранилище недоступно — ключ просто останется до перезаписи.
  }
}

const InviteForm = memo(function InviteForm(props: {
  submitting: boolean;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
}) {
  return (
    <form className="form-card business-invite-form" onSubmit={props.onSubmit}>
      <div className="field business-invite-email">
        <label htmlFor="business-invite-email">Email клиента</label>
        <input id="business-invite-email" name="email" type="email" placeholder="client@company.com…" autoComplete="off" spellCheck={false} />
        <div className="sub">Не укажете email — готовая ссылка сразу скопируется.</div>
      </div>
      <div className="field">
        <label htmlFor="business-invite-days">Срок действия</label>
        <div className="business-input-unit">
          <input id="business-invite-days" name="days" type="number" min={1} max={30} defaultValue={7} required />
          <span>дней</span>
        </div>
      </div>
      <div className="field">
        <label htmlFor="business-invite-discount">Базовая скидка</label>
        <div className="business-input-unit">
          <input id="business-invite-discount" name="discount" type="number" min={0} max={100} defaultValue={0} required />
          <span>%</span>
        </div>
      </div>
      <button className="btn business-invite-submit" type="submit" disabled={props.submitting}>
        {props.submitting ? "Создаём…" : "Создать инвайт"}
      </button>
    </form>
  );
});

export const CLIENTS_HEAD = (
  <thead>
    <tr>
      <th className="left">клиент</th>
      <th className="business-col-discount">базовая скидка</th>
      <th className="business-col-balance">баланс</th>
      <th className="business-col-status">аккаунт</th>
      <th className="business-col-status">доставка цены</th>
      <th />
    </tr>
  </thead>
);

const INVITES_HEAD = (
  <thead>
    <tr>
      <th className="left">получатель</th>
      <th>базовая скидка</th>
      <th>статус</th>
      <th>доставка</th>
      <th>истекает</th>
      <th>действия</th>
    </tr>
  </thead>
);

export function ClientRow(props: {
  user: BusinessUser;
  busy: boolean;
  onPricing: (user: BusinessUser) => void;
}): ReactElement {
  const { user } = props;
  const discount = discountFromMultiplierBp(user.multiplier_bp);
  const engine = engineStatusPresentation(user.engine_account_status);
  const pricing = pricingSyncPresentation(user.pricing_sync_status);
  return (
    <tr>
      <td className="left">
        <b>{user.email}</b>
        <div className="sub mono">{user.id}</div>
      </td>
      <td className="business-col-discount">
        <div className="business-discount-value">{discount == null ? "—" : `${discount}%`}</div>
        <div className="sub">для всех провайдеров</div>
      </td>
      <td className="business-balance business-col-balance">{money(user.balance_usd)}</td>
      <td className="business-col-status">
        <Pill kind={engine.kind}>{engine.label}</Pill>
      </td>
      <td className="business-col-status">
        <Pill kind={pricing.kind}>{pricing.label}</Pill>
        {user.pricing_sync_error ? <div className="sub business-sync-error">{user.pricing_sync_error}</div> : null}
      </td>
      <td className="business-client-action">
        <button className="btn" disabled={props.busy} onClick={() => props.onPricing(user)}>
          Настроить скидки
        </button>
      </td>
    </tr>
  );
}

const MemoClientRow = memo(ClientRow);

const InviteRow = memo(function InviteRow(props: {
  invite: BusinessInvite;
  busyCopy: boolean;
  busyResend: boolean;
  busyRevoke: boolean;
  onCopy: (id: string) => void;
  onResend: (id: string) => void;
  onRevoke: (id: string) => void;
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
      <td>{invite.discount_percent == null ? "—" : `${invite.discount_percent}%`}</td>
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


export default function BusinessPage() {
  const [requestedOffset, setOffset] = useState(0);
  const [busyIds, setBusyIds] = useState<ReadonlySet<string>>(() => new Set<string>());
  const [discountTarget, setDiscountTarget] = useState<DiscountDialogTarget | null>(null);
  const { data: result, isLoading } = useResources<{
    invites: BusinessInvitesPage;
    users: BusinessUsersPage;
  }>({
    invites: `/admin/business-invites?limit=${INVITES_LIMIT}`,
    users: `/admin/users?limit=${CLIENT_LIMIT}&offset=${requestedOffset}&customer_type=b2b`,
  });

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
  const offset = clientTotal > 0 && requestedOffset >= clientTotal
    ? Math.max(0, Math.floor((clientTotal - 1) / CLIENT_LIMIT) * CLIENT_LIMIT)
    : requestedOffset;

  useEffect(() => {
    if (result.users && offset !== requestedOffset) startTransition(() => setOffset(offset));
  }, [offset, requestedOffset, result.users]);

  const submitInvite = useCallback(
    async (event: FormEvent<HTMLFormElement>) => {
      event.preventDefault();
      const form = event.currentTarget;
      const data = new FormData(form);
      const email = String(data.get("email") ?? "").trim();
      const expiresInDays = Number(data.get("days"));
      const reason = PANEL_REASON;
      const discountPercent = Number(data.get("discount"));
      // Подпись параметров + сохранённый ключ: безопасный повтор после сетевой ошибки.
      const signature = [email, expiresInDays, reason, discountPercent].join("\n");
      const idempotencyKey = reuseIdempotencyKey(INVITE_PENDING_KEY, signature);
      const payload: Record<string, unknown> = { discountPercent, expiresInDays, reason, idempotencyKey };
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
      } catch (error) {
        toast(`${errorText(error)} — безопасный ключ повтора сохранён.`, "bad");
      } finally {
        dropBusy("submit");
      }
    },
    [addBusy, dropBusy],
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
      } catch (error) {
        toast(errorText(error), "bad");
      } finally {
        dropBusy(`revoke:${id}`);
      }
    },
    [addBusy, dropBusy],
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
      } catch (error) {
        toast(`${errorText(error)} — безопасный ключ повтора сохранён.`, "bad");
      } finally {
        dropBusy(`resend:${id}`);
      }
    },
    [addBusy, dropBusy],
  );

  const openClientDiscounts = useCallback((user: BusinessUser) => {
    const currentDiscount = discountFromMultiplierBp(user.multiplier_bp);
    setDiscountTarget({
      userId: user.id,
      title: `Скидки · ${user.email}`,
      defaultPercent: currentDiscount == null ? 0 : Math.round(currentDiscount),
    });
  }, []);

  if (isLoading && Object.values(result).every((value) => value === undefined)) {
    return (
      <>
        <PageHead title="B2B" sub="данные загружаются, навигация уже доступна" />
        <LoadingGrid />
      </>
    );
  }

  const invites = result.invites?.invites ?? [];

  return (
    <div className="business-page">
      <PageHead
        title="B2B"
        sub="Договорные скидки B2B-клиентов и приглашения"
        badge={<Pill kind="ok">{count(clientTotal, "клиент", "клиента", "клиентов")}</Pill>}
      />

      <section className="business-section" aria-labelledby="business-clients-title">
        <div className="business-section-head">
          <div>
            <h2 id="business-clients-title">B2B-клиенты</h2>
            <p>Текущие условия и состояние их доставки в engine.</p>
          </div>
          <Pill>{count(clientTotal, "клиент", "клиента", "клиентов")}</Pill>
        </div>
        <TableCard>
          <table className="business-client-table">
            {CLIENTS_HEAD}
            <tbody>
              {clients.length ? clients.map((user) => (
                <MemoClientRow key={user.id} user={user} busy={busyIds.has(`pricing:${user.id}`)} onPricing={openClientDiscounts} />
              )) : <EmptyRow columns={6} />}
            </tbody>
          </table>
        </TableCard>
        <Pager offset={offset} limit={CLIENT_LIMIT} total={clientTotal} onPage={setOffset} />
      </section>

      <section className="business-section" aria-labelledby="business-invite-title">
        <div className="business-section-head">
          <div>
            <h2 id="business-invite-title">Приглашения</h2>
            <p>Создайте доступ и отслеживайте доставку приглашения.</p>
          </div>
          <Pill>{count(invites.length, "инвайт", "инвайта", "инвайтов")}</Pill>
        </div>
        <InviteForm submitting={busyIds.has("submit")} onSubmit={submitInvite} />
        <div className="business-invite-list-head">Последние приглашения</div>
        <TableCard>
          <table className="business-invite-table">
            {INVITES_HEAD}
            <tbody>
              {invites.length ? invites.map((invite) => (
                <InviteRow key={invite.id} invite={invite} busyCopy={busyIds.has(`copy:${invite.id}`)} busyResend={busyIds.has(`resend:${invite.id}`)} busyRevoke={busyIds.has(`revoke:${invite.id}`)} onCopy={copyInviteLink} onResend={resendInvite} onRevoke={revokeInvite} />
              )) : <EmptyRow columns={6} />}
            </tbody>
          </table>
        </TableCard>
      </section>

      <DiscountDialog
        target={discountTarget}
        reason={PANEL_REASON}
        onClose={() => setDiscountTarget(null)}
      />
    </div>
  );
}
