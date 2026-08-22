"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import { api, ApiError, formatBps, formatDate, formatUsd, type InviteRow, type TeamRow } from "@/lib/api";
import { Badge, Button, Card, CopyButton, EmptyState, Field, Input, Loading, Notice, Table } from "@/components/ui";
import { localeFor, useI18n } from "@/components/i18n";

type TeamResponse = {
  platformCommissionBps: number;
  teamOverrideMaxBps: number;
  items: TeamRow[];
};
type InviteResponse = { items: InviteRow[] };
type CreatedInvite = InviteRow & { overrideBps: number; teamOverrideMaxBps: number };

function inviteState(invite: InviteRow): { label: [string, string]; tone: "green" | "yellow" | "neutral" } {
  if (invite.consumedAt) return { label: ["Joined", "Присоединился"], tone: "green" };
  if (invite.expiresAt && new Date(invite.expiresAt).getTime() <= Date.now()) return { label: ["Expired", "Истёк"], tone: "neutral" };
  return { label: ["Waiting", "Ожидает"], tone: "yellow" };
}

/** Convert 0..20 percent with at most two decimals to integer basis points. */
export function percentToTeamBps(input: string): number | null {
  const match = /^(0|[1-9]|1\d|20)(?:\.(\d{1,2}))?$/.exec(input.trim());
  if (!match) return null;
  const whole = Number(match[1]);
  const fraction = Number((match[2] ?? "").padEnd(2, "0"));
  const bps = whole * 100 + fraction;
  return bps <= 2_000 ? bps : null;
}

function bpsInput(bps: number): string {
  return String(bps / 100);
}

function memberIdentity(member: TeamRow): string {
  if (member.email) return member.email;
  if (member.telegramUsername) return `@${member.telegramUsername}`;
  return member.displayName ?? `partner-${member.id.slice(0, 8)}`;
}

export default function TeamPage() {
  const { lang, t } = useI18n();
  const locale = localeFor(lang);
  const [teamData, setTeamData] = useState<TeamResponse | null>(null);
  const [invites, setInvites] = useState<InviteRow[] | null>(null);
  const [telegramUsername, setTelegramUsername] = useState("");
  const [overridePercent, setOverridePercent] = useState("10");
  const [memberCeilingPercent, setMemberCeilingPercent] = useState("20");
  const [created, setCreated] = useState<CreatedInvite | null>(null);
  const [editing, setEditing] = useState<TeamRow | null>(null);
  const [editOverridePercent, setEditOverridePercent] = useState("");
  const [editCeilingPercent, setEditCeilingPercent] = useState("");
  const [busy, setBusy] = useState(false);
  const [editBusy, setEditBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  const load = useCallback(async () => {
    const [teamResponse, inviteResponse] = await Promise.all([
      api<TeamResponse>("/v1/partner/team"),
      api<InviteResponse>("/v1/partner/invites"),
    ]);
    setTeamData(teamResponse);
    setInvites(inviteResponse.items);
    setOverridePercent((current) => current === "10" ? bpsInput(Math.min(1_000, teamResponse.teamOverrideMaxBps)) : current);
    setMemberCeilingPercent((current) => current === "20" ? bpsInput(teamResponse.teamOverrideMaxBps) : current);
    setEditing((current) => current ? teamResponse.items.find((member) => member.id === current.id) ?? null : null);
  }, []);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        await load();
      } catch (err) {
        if (!cancelled) setError(err instanceof ApiError ? err.message : t("Failed to load your team.", "Не удалось загрузить команду."));
      }
    })();
    return () => { cancelled = true; };
  }, [load, t]);

  const pendingInvites = useMemo(
    () => (invites ?? []).filter((invite) => !invite.consumedAt && (!invite.expiresAt || new Date(invite.expiresAt).getTime() > Date.now())).length,
    [invites],
  );
  const team = teamData?.items ?? null;
  const maximumBps = teamData?.teamOverrideMaxBps ?? 0;

  function parseControls(overrideValue: string, ceilingValue: string): { overrideBps: number; teamOverrideMaxBps: number } | null {
    const overrideBps = percentToTeamBps(overrideValue);
    const teamOverrideMaxBps = percentToTeamBps(ceilingValue);
    if (overrideBps === null || teamOverrideMaxBps === null || overrideBps > maximumBps || teamOverrideMaxBps > maximumBps) {
      setError(t(
        `Both values must be between 0% and ${formatBps(maximumBps)}, with at most two decimal places.`,
        `Оба значения должны быть от 0% до ${formatBps(maximumBps)}, не более двух знаков после запятой.`,
      ));
      return null;
    }
    return { overrideBps, teamOverrideMaxBps };
  }

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
    const controls = parseControls(overridePercent, memberCeilingPercent);
    if (!controls) return;
    setBusy(true);
    try {
      const result = await api<{
        code: string; inviteUrl: string; telegramUsername: string | null; commissionBps: number;
        overrideBps: number; teamOverrideMaxBps: number; expiresAt: string | null;
      }>("/v1/partner/team/invites", {
        method: "POST",
        body: { telegramUsername: username, ...controls },
      });
      const invite: CreatedInvite = { ...result, consumedAt: null, createdAt: new Date().toISOString() };
      setCreated(invite);
      setTelegramUsername("");
      setSuccess(t("Invite created. Send the link to your team member.", "Приглашение создано. Отправьте ссылку участнику команды."));
      await load();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("Could not create the invite.", "Не удалось создать приглашение."));
    } finally {
      setBusy(false);
    }
  }

  function startEditing(member: TeamRow) {
    setEditing(member);
    setEditOverridePercent(bpsInput(member.overrideBps));
    setEditCeilingPercent(bpsInput(member.teamOverrideMaxBps));
    setError(null);
    setSuccess(null);
    window.requestAnimationFrame(() => document.getElementById("team-member-override")?.focus());
  }

  async function saveMember(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!editing) return;
    setError(null);
    setSuccess(null);
    const controls = parseControls(editOverridePercent, editCeilingPercent);
    if (!controls) return;
    setEditBusy(true);
    try {
      await api(`/v1/partner/team/${editing.id}`, { method: "PATCH", body: controls });
      setSuccess(t("Team settings saved.", "Настройки участника сохранены."));
      await load();
      setEditing(null);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("Could not save team settings.", "Не удалось сохранить настройки участника."));
    } finally {
      setEditBusy(false);
    }
  }

  if (error && !teamData && !invites) return <Notice kind="error">{error}</Notice>;
  if (!teamData || !team || !invites) return <Loading label={t("Loading your team…", "Загружаем команду…")} />;

  return <>
    <h1 className="page-title">{t("Team", "Команда")}</h1>
    <p className="page-sub">
      {t(
        "Invite partners into your team. The platform gives each new member a 10% direct commission by default; you choose only your override from that commission and their maximum team override, within your personal limit.",
        "Приглашайте партнёров в команду. Платформа по умолчанию даёт каждому новому участнику 10% прямой комиссии; вы выбираете только свою надбавку с этой комиссии и его лимит для команды — в пределах вашего личного максимума.",
      )}
    </p>

    <div className="stat-grid team-stats">
      <div className="stat-card"><div className="stat-label">{t("Your team limit", "Ваш лимит команды")}</div><div className="stat-value green">{formatBps(maximumBps)}</div><div className="stat-foot">{t("platform hard maximum 20%", "глобальный максимум платформы 20%")}</div></div>
      <div className="stat-card"><div className="stat-label">{t("Member direct rate", "Прямая ставка участника")}</div><div className="stat-value">{formatBps(teamData.platformCommissionBps)}</div><div className="stat-foot">{t("set by the platform, not the inviter", "задаётся платформой, не приглашающим")}</div></div>
      <div className="stat-card"><div className="stat-label">{t("Team members", "Участники команды")}</div><div className="stat-value">{team.length.toLocaleString(locale)}</div><div className="stat-foot">{t("direct members only", "только прямые участники")}</div></div>
      <div className="stat-card"><div className="stat-label">{t("Pending invites", "Ожидают входа")}</div><div className="stat-value">{pendingInvites.toLocaleString(locale)}</div><div className="stat-foot">{t("valid for 30 days", "действуют 30 дней")}</div></div>
    </div>

    <div className="stack">
      {error ? <Notice kind="error">{error}</Notice> : null}
      {success ? <Notice kind="success">{success}</Notice> : null}

      <Card title={t("Invite a team member", "Пригласить участника команды")} sub={t(
        "The invite is bound to their Telegram username. Percentages are exact settings for this member and may be edited later.",
        "Приглашение привязано к Telegram-имени. Проценты — точные настройки этого участника, их можно изменить позже.",
      )}>
        <form onSubmit={createInvite} className="team-invite-form" noValidate>
          <Field label={t("Telegram username", "Имя пользователя Telegram")} htmlFor="team-invite-telegram" hint={t("For example: @partner_name", "Например: @partner_name")}>
            <Input id="team-invite-telegram" value={telegramUsername} onChange={(event) => setTelegramUsername(event.target.value)} placeholder="@partner_name…" autoComplete="off" spellCheck={false} disabled={busy} />
          </Field>
          <Field label={t("Your override", "Ваша надбавка")} htmlFor="team-invite-override" hint={t("Percent of the member's direct commission", "Процент от прямой комиссии участника")}>
            <div className="input-suffix"><Input id="team-invite-override" type="number" min={0} max={maximumBps / 100} step={0.01} value={overridePercent} onChange={(event) => setOverridePercent(event.target.value)} disabled={busy} inputMode="decimal" autoComplete="off" /><span aria-hidden>%</span></div>
          </Field>
          <Field label={t("Member's team limit", "Лимит участника для команды")} htmlFor="team-invite-ceiling" hint={t("Maximum they may assign to their own members", "Максимум, который он сможет назначать своей команде")}>
            <div className="input-suffix"><Input id="team-invite-ceiling" type="number" min={0} max={maximumBps / 100} step={0.01} value={memberCeilingPercent} onChange={(event) => setMemberCeilingPercent(event.target.value)} disabled={busy} inputMode="decimal" autoComplete="off" /><span aria-hidden>%</span></div>
          </Field>
          <div className="team-invite-action"><Button type="submit" loading={busy}>{t("Create invite", "Создать приглашение")}</Button></div>
        </form>
        <p className="team-control-note">{t(
          `Both values are limited to ${formatBps(maximumBps)} for your account and never exceed the platform maximum of 20%. You cannot change the member's platform-funded direct rate.`,
          `Оба значения ограничены ${formatBps(maximumBps)} для вашего аккаунта и никогда не превышают глобальные 20%. Прямую ставку участника от платформы вы не меняете.`,
        )}</p>
        {created ? <div className="created-invite">
          <div><strong>{t("Invite ready", "Приглашение готово")}</strong><span className="field-hint">{formatBps(created.commissionBps)} {t("direct", "прямая")} · {formatBps(created.overrideBps)} {t("your override", "ваша надбавка")} · {formatBps(created.teamOverrideMaxBps)} {t("team limit", "лимит команды")} · {formatDate(created.expiresAt, locale)}</span></div>
          <div className="reflink-row"><Input readOnly value={created.inviteUrl} aria-label={t("Created invite link", "Созданная ссылка-приглашение")} onFocus={(event) => event.currentTarget.select()} /><CopyButton value={created.inviteUrl} label={t("Copy link", "Копировать ссылку")} /></div>
        </div> : null}
      </Card>

      {editing ? <Card className="team-editor" title={t(`Edit ${memberIdentity(editing)}`, `Настроить ${memberIdentity(editing)}`)} sub={t(
        "Changing the member's team limit may also lower settings they already delegated below them, so the whole subtree remains valid.",
        "Снижение лимита участника может также уменьшить уже выданные им настройки ниже по команде, чтобы всё дерево оставалось корректным.",
      )}>
        <form className="team-edit-form" onSubmit={saveMember} noValidate>
          <Field label={t("Your override", "Ваша надбавка")} htmlFor="team-member-override" hint={t("Percent of this member's direct commission", "Процент от прямой комиссии этого участника")}>
            <div className="input-suffix"><Input id="team-member-override" type="number" min={0} max={maximumBps / 100} step={0.01} value={editOverridePercent} onChange={(event) => setEditOverridePercent(event.target.value)} disabled={editBusy} inputMode="decimal" autoComplete="off" /><span aria-hidden>%</span></div>
          </Field>
          <Field label={t("Member's team limit", "Лимит участника для команды")} htmlFor="team-member-ceiling" hint={t("Maximum they may delegate", "Максимум, который он может делегировать")}>
            <div className="input-suffix"><Input id="team-member-ceiling" type="number" min={0} max={maximumBps / 100} step={0.01} value={editCeilingPercent} onChange={(event) => setEditCeilingPercent(event.target.value)} disabled={editBusy} inputMode="decimal" autoComplete="off" /><span aria-hidden>%</span></div>
          </Field>
          <div className="team-edit-actions"><Button type="submit" loading={editBusy}>{t("Save settings", "Сохранить")}</Button><Button type="button" variant="ghost" disabled={editBusy} onClick={() => setEditing(null)}>{t("Cancel", "Отмена")}</Button></div>
        </form>
      </Card> : null}

      <Card title={t("Your team", "Ваша команда")} sub={t(
        "Your override is calculated from each member's direct commission, not from their top-ups or list-price usage.",
        "Ваша надбавка считается от прямой комиссии каждого участника, а не от его пополнений или цены по прайсу.",
      )}>
        {team.length === 0 ? <EmptyState title={t("No team members yet", "Пока нет участников команды")}>{t("Create an invite above to start your team.", "Создайте приглашение выше, чтобы начать команду.")}</EmptyState> : <Table label={t("Team members", "Участники команды")} head={<>
          <th>{t("Partner", "Партнёр")}</th><th>{t("Status", "Статус")}</th><th className="num">{t("Direct rate", "Прямая ставка")}</th><th className="num">{t("Your override", "Ваша надбавка")}</th><th className="num">{t("Their team limit", "Их лимит команды")}</th><th className="num">{t("Referrals", "Рефералы")}</th><th className="num">{t("They earned", "Они заработали")}</th><th className="num">{t("You earned", "Вы заработали")}</th><th><span className="sr-only">{t("Actions", "Действия")}</span></th>
        </>}>
          {team.map((member) => <tr key={member.id}>
            <td><span className="identity-email" title={memberIdentity(member)}>{memberIdentity(member)}</span>{member.email && member.telegramUsername ? <div className="identity-secondary">@{member.telegramUsername}</div> : null}</td>
            <td><Badge tone={member.status === "active" ? "green" : "neutral"}>{member.status}</Badge></td>
            <td className="num">{formatBps(member.commissionBps)}</td><td className="num">{formatBps(member.overrideBps)}</td><td className="num">{formatBps(member.teamOverrideMaxBps)}</td><td className="num">{member.referredUsers.toLocaleString(locale)}</td><td className="num">{formatUsd(member.netNano)}</td><td className="num team-earned">{formatUsd(member.myOverrideNetNano)}</td>
            <td><Button type="button" size="sm" variant="ghost" onClick={() => startEditing(member)}>{t("Edit", "Изменить")}</Button></td>
          </tr>)}
        </Table>}
      </Card>

      <Card title={t("Invitations", "Приглашения")} sub={t("Each unused invite expires after 30 days and can be used once.", "Неиспользованное приглашение действует 30 дней и используется один раз.")}>
        {invites.length === 0 ? <EmptyState title={t("No invitations yet", "Пока нет приглашений")} /> : <Table label={t("Team invitations", "Приглашения в команду")} head={<>
          <th>{t("Telegram", "Telegram")}</th><th>{t("Status", "Статус")}</th><th className="num">{t("Direct rate", "Прямая ставка")}</th><th className="num">{t("Your override", "Ваша надбавка")}</th><th className="num">{t("Team limit", "Лимит команды")}</th><th>{t("Expires", "Истекает")}</th><th>{t("Link", "Ссылка")}</th>
        </>}>
          {invites.map((invite) => {
            const state = inviteState(invite);
            return <tr key={invite.code}><td className="mono">{invite.telegramUsername ? `@${invite.telegramUsername}` : "—"}</td><td><Badge tone={state.tone}>{t(state.label[0], state.label[1])}</Badge></td><td className="num">{formatBps(invite.commissionBps)}</td><td className="num">{formatBps(invite.overrideBps)}</td><td className="num">{formatBps(invite.teamOverrideMaxBps)}</td><td>{formatDate(invite.expiresAt, locale)}</td><td>{!invite.consumedAt ? <CopyButton value={invite.inviteUrl} label={t("Copy", "Копировать")} /> : "—"}</td></tr>;
          })}
        </Table>}
      </Card>
    </div>
  </>;
}
