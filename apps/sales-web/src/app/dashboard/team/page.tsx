"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import {
  api,
  ApiError,
  formatBps,
  formatDate,
  formatUsd,
  type InviteRow,
  type TeamRow,
} from "@/lib/api";
import {
  Badge,
  Button,
  Card,
  CopyButton,
  EmptyState,
  Field,
  Input,
  Loading,
  Notice,
  Table,
} from "@/components/ui";
import { localeFor, useI18n } from "@/components/i18n";
import { usePartner } from "@/components/partner-context";

type TeamResponse = { items: TeamRow[] };
type InviteResponse = { items: InviteRow[] };
type CreatedInvite = InviteRow & { overrideBps?: number };

function inviteState(invite: InviteRow): { label: [string, string]; tone: "green" | "yellow" | "neutral" } {
  if (invite.consumedAt) return { label: ["Joined", "Присоединился"], tone: "green" };
  if (invite.expiresAt && new Date(invite.expiresAt).getTime() <= Date.now()) {
    return { label: ["Expired", "Истёк"], tone: "neutral" };
  }
  return { label: ["Waiting", "Ожидает"], tone: "yellow" };
}

/** Convert a displayed percentage to integer basis points without floating-point rounding. */
function percentToBps(input: string): number | null {
  const match = /^(0|[1-9]\d{0,2})(?:\.(\d{1,2}))?$/.exec(input.trim());
  if (!match) return null;
  const whole = Number(match[1]);
  const fraction = Number((match[2] ?? "").padEnd(2, "0"));
  const bps = whole * 100 + fraction;
  return bps <= 10_000 ? bps : null;
}

export default function TeamPage() {
  const { lang, t } = useI18n();
  const locale = localeFor(lang);
  const partner = usePartner();
  const [team, setTeam] = useState<TeamRow[] | null>(null);
  const [invites, setInvites] = useState<InviteRow[] | null>(null);
  const [telegramUsername, setTelegramUsername] = useState("");
  const [commissionPercent, setCommissionPercent] = useState(() => String(partner.commissionBps / 100));
  const [created, setCreated] = useState<CreatedInvite | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  const load = useCallback(async () => {
    const [teamResponse, inviteResponse] = await Promise.all([
      api<TeamResponse>("/v1/partner/team"),
      api<InviteResponse>("/v1/partner/invites"),
    ]);
    setTeam(teamResponse.items);
    setInvites(inviteResponse.items);
  }, []);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        await load();
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof ApiError ? err.message : t("Failed to load your team.", "Не удалось загрузить команду."));
        }
      }
    })();
    return () => { cancelled = true; };
  }, [load, t]);

  const pendingInvites = useMemo(
    () => (invites ?? []).filter((invite) => !invite.consumedAt && (!invite.expiresAt || new Date(invite.expiresAt).getTime() > Date.now())).length,
    [invites],
  );

  async function createInvite(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setError(null);
    setSuccess(null);
    setCreated(null);
    const username = telegramUsername.trim();
    if (!/^@?[A-Za-z0-9_]{5,32}$/.test(username)) {
      setError(t("Enter a valid Telegram username.", "Введите корректное имя пользователя Telegram."));
      return;
    }
    const commissionBps = percentToBps(commissionPercent);
    if (commissionBps === null || commissionBps > partner.commissionBps) {
      setError(t(
        `The sub-partner rate must be from 0% to ${formatBps(partner.commissionBps)} (up to two decimals).`,
        `Ставка суб-партнёра должна быть от 0% до ${formatBps(partner.commissionBps)} (до двух знаков).`,
      ));
      return;
    }
    setBusy(true);
    try {
      const result = await api<{
        code: string;
        inviteUrl: string;
        telegramUsername: string | null;
        commissionBps: number | null;
        overrideBps?: number;
        expiresAt: string | null;
      }>("/v1/partner/invites", {
        method: "POST",
        body: { telegramUsername: username, commissionBps },
      });
      const invite: CreatedInvite = {
        ...result,
        consumedAt: null,
        createdAt: new Date().toISOString(),
      };
      setCreated(invite);
      setTelegramUsername("");
      setSuccess(t("Invite created. Send the link to your sub-partner.", "Приглашение создано. Отправьте ссылку суб-партнёру."));
      await load();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("Could not create the invite.", "Не удалось создать приглашение."));
    } finally {
      setBusy(false);
    }
  }

  if (error && !team && !invites) return <Notice kind="error">{error}</Notice>;
  if (!team || !invites) return <Loading label={t("Loading your team…", "Загружаем команду…")} />;

  return (
    <>
      <h1 className="page-title">{t("Team", "Команда")}</h1>
      <p className="page-sub">
        {t(
          "Invite sub-partners. They earn on their own referrals, and you receive an override on each direct commission they earn.",
          "Приглашайте суб-партнёров. Они получают комиссию со своих рефералов, а вы — надбавку с каждой их прямой комиссии.",
        )}
      </p>

      <div className="stat-grid team-stats">
        <div className="stat-card">
          <div className="stat-label">{t("Your override", "Ваша надбавка")}</div>
          <div className="stat-value green">{formatBps(partner.subCommissionBps)}</div>
          <div className="stat-foot">{t("of each sub-partner's direct commission", "с прямой комиссии суб-партнёра")}</div>
        </div>
        <div className="stat-card">
          <div className="stat-label">{t("Team members", "Участники команды")}</div>
          <div className="stat-value">{team.length}</div>
          <div className="stat-foot">{t("active and historical members", "активные и бывшие участники")}</div>
        </div>
        <div className="stat-card">
          <div className="stat-label">{t("Pending invites", "Ожидают входа")}</div>
          <div className="stat-value">{pendingInvites}</div>
          <div className="stat-foot">{t("valid for 30 days", "действуют 30 дней")}</div>
        </div>
      </div>

      <div className="stack">
        {error ? <Notice kind="error">{error}</Notice> : null}
        {success ? <Notice kind="success">{success}</Notice> : null}

        <Card
          title={t("Invite a sub-partner", "Пригласить суб-партнёра")}
          sub={t(
            "The link is bound to their Telegram username. The account opens after they confirm with Telegram.",
            "Ссылка привязана к имени пользователя Telegram. Аккаунт откроется после входа через Telegram.",
          )}
        >
          <form onSubmit={createInvite} className="team-invite-form">
            <Field
              label={t("Telegram username", "Имя пользователя Telegram")}
              htmlFor="team-invite-telegram"
              hint={t("For example: @partner_name", "Например: @partner_name")}
            >
              <Input
                id="team-invite-telegram"
                value={telegramUsername}
                onChange={(event) => setTelegramUsername(event.target.value)}
                placeholder="@username"
                autoComplete="off"
                spellCheck={false}
                disabled={busy}
              />
            </Field>
            <Field
              label={t("Their direct rate", "Их прямая ставка")}
              htmlFor="team-invite-rate"
              hint={t(`Maximum ${formatBps(partner.commissionBps)}.`, `Максимум ${formatBps(partner.commissionBps)}.`)}
            >
              <div className="input-suffix">
                <Input
                  id="team-invite-rate"
                  type="number"
                  min={0}
                  max={partner.commissionBps / 100}
                  step={0.01}
                  value={commissionPercent}
                  onChange={(event) => setCommissionPercent(event.target.value)}
                  disabled={busy}
                  inputMode="decimal"
                  autoComplete="off"
                />
                <span aria-hidden>%</span>
              </div>
            </Field>
            <div className="team-invite-action">
              <Button type="submit" loading={busy}>{t("Create invite", "Создать приглашение")}</Button>
            </div>
          </form>
          {created ? (
            <div className="created-invite">
              <div>
                <strong>{t("Invite ready", "Приглашение готово")}</strong>
                <span className="field-hint">
                  {formatBps(created.commissionBps)} {t("direct rate", "прямая ставка")} · {formatDate(created.expiresAt, locale)}
                </span>
              </div>
              <div className="reflink-row">
                <Input readOnly value={created.inviteUrl} aria-label={t("Created invite link", "Созданная ссылка-приглашение")} onFocus={(event) => event.currentTarget.select()} />
                <CopyButton value={created.inviteUrl} label={t("Copy link", "Копировать ссылку")} />
              </div>
            </div>
          ) : null}
        </Card>

        <Card
          title={t("Your sub-partners", "Ваши суб-партнёры")}
          sub={t(
            "Your override is calculated from the sub-partner's direct commission, not from their top-ups or list-price usage.",
            "Ваша надбавка считается от прямой комиссии суб-партнёра, а не от пополнений или цены по прайсу.",
          )}
        >
          {team.length === 0 ? (
            <EmptyState title={t("No sub-partners yet", "Пока нет суб-партнёров")}>
              {t("Create an invite above to start your team.", "Создайте приглашение выше, чтобы начать команду.")}
            </EmptyState>
          ) : (
            <Table
              head={
                <>
                  <th>{t("Partner", "Партнёр")}</th>
                  <th>{t("Status", "Статус")}</th>
                  <th className="num">{t("Direct rate", "Прямая ставка")}</th>
                  <th className="num">{t("Referred users", "Рефералы")}</th>
                  <th className="num">{t("They earned", "Они заработали")}</th>
                  <th className="num">{t("Your override", "Ваша надбавка")}</th>
                </>
              }
            >
              {team.map((member) => (
                <tr key={member.id}>
                  <td>
                    <span className="mono">{member.telegramUsername ? `@${member.telegramUsername}` : member.displayName ?? member.email ?? `partner-${member.id.slice(0, 8)}`}</span>
                  </td>
                  <td><Badge tone={member.status === "active" ? "green" : "neutral"}>{member.status}</Badge></td>
                  <td className="num">{formatBps(member.commissionBps)}</td>
                  <td className="num">{member.referredUsers}</td>
                  <td className="num">{formatUsd(member.netNano)}</td>
                  <td className="num" style={{ color: "var(--accent-strong)", fontWeight: 700 }}>{formatUsd(member.myOverrideNetNano)}</td>
                </tr>
              ))}
            </Table>
          )}
        </Card>

        <Card
          title={t("Invitations", "Приглашения")}
          sub={t("Each unused invite expires after 30 days and can be used once.", "Неиспользованное приглашение действует 30 дней и используется один раз.")}
        >
          {invites.length === 0 ? (
            <EmptyState title={t("No invitations yet", "Пока нет приглашений")} />
          ) : (
            <Table
              head={
                <>
                  <th>{t("Telegram", "Telegram")}</th>
                  <th>{t("Status", "Статус")}</th>
                  <th className="num">{t("Direct rate", "Прямая ставка")}</th>
                  <th>{t("Expires", "Истекает")}</th>
                  <th>{t("Link", "Ссылка")}</th>
                </>
              }
            >
              {invites.map((invite) => {
                const state = inviteState(invite);
                return (
                  <tr key={invite.code}>
                    <td className="mono">{invite.telegramUsername ? `@${invite.telegramUsername}` : "—"}</td>
                    <td><Badge tone={state.tone}>{t(state.label[0], state.label[1])}</Badge></td>
                    <td className="num">{formatBps(invite.commissionBps)}</td>
                    <td>{formatDate(invite.expiresAt, locale)}</td>
                    <td><CopyButton value={invite.inviteUrl} label={t("Copy", "Копировать")} /></td>
                  </tr>
                );
              })}
            </Table>
          )}
        </Card>
      </div>
    </>
  );
}
