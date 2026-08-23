"use client";

import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties, type FormEvent, type ReactNode } from "react";
import { useI18n } from "@/components/i18n-provider";
import {
  api,
  ApiError,
  type ReferralActiveSnapshot,
  type ReferralAuthorityInput,
  type ReferralSnapshot,
  type ReferralTeamMember,
} from "@/lib/api";
import { DASHBOARD_PROVIDERS, fallbackProvider } from "@/lib/providers";
import { CopyButton, formatNanoUsd, interpolate, PageHeading } from "./shared";

// The partner cabinet (partners.apitoken.sale) is the design reference for this view:
// uppercase page/card titles, one joined stat strip, the commission formula, the referral
// link, bullet explainers and bordered tables. Layout classes live in dashboard.css (.rp-*).
const PARTNER_SITE_ORIGIN = "https://apitoken.sale";
const TABS = ["overview", "referrals", "team", "payouts", "docs"] as const;
type ReferralTab = typeof TABS[number];
type Language = "en" | "ru";

const copy = {
  en: {
    eyebrow: "Partner program", title: "Referrals", subtitle: "Your clients, Team, earnings and payouts — inside the same account you use for the API.",
    loading: "Loading partner data…", loadError: "Partner data is temporarily unavailable.", retry: "Try again",
    ordinaryTitle: "Grow with apiToken.sale", ordinarySubtitle: "Partner access is approved individually. Here are the standard terms.", ordinaryBody: "The partner program is enabled manually. Standard terms start at a 10% commission on paid usage from your referred clients. Your referrals and Team members are existing apiToken.sale accounts and are identified by their account email.",
    ordinaryPoint1: "Commission only on eligible paid usage", ordinaryPoint2: "Provider-level earnings and transparent payout periods", ordinaryPoint3: "A Team hierarchy with a retained share capped at 20%",
    requestAccess: "Request partner access", contactHint: "The button opens a conversation with @bozinodev in Telegram. Your Dashboard account remains the only program identity.",
    disabledTitle: "Partner access is paused", disabledBody: "Your account is still intact, but partner actions are disabled. Contact support to clarify or restore access.", contact: "Contact @bozinodev",
    overview: "Overview", referrals: "Referrals", team: "Team", requests: "Requests", payouts: "Payouts", docs: "Docs",
    available: "Available", earned30: "Net earned · 30 days", direct: "Direct earnings", teamIncome: "Retained Team share", payable: "Payable", fixedRate: "Your platform commission", fixedRateHint: "Set only by apiToken.sale",
    chartTitle: "Earnings over time", chartWindow: "Last 30 days", noEarnings: "No partner earnings in this period yet.", providerSummary: "Period summary", providerCards: "Earnings by provider", providerCardsSub: "The same provider view as Usage, calculated only from eligible paid referral usage.", ready: "Active", events: "events", earned: "Earned", spend: "Paid usage", adjustments: "Adjustments", net: "Net", dailyAverage: "Daily average", peakDay: "Peak day",
    programTerms: "How the Team share works", termsBody: "Your Team share is retained from a member’s fixed platform commission; it is not added on top. With $100 of eligible spend and a 10% commission, the pool is $10. A 20% retained share gives $8 to the member and $2 to the parent — the total remains $10.",
    referralList: "Referred accounts", referralListSub: "Accounts are identified by their current apiToken.sale login email. Paid usage excludes free platform credit.", searchReferrals: "Search by email", searchPlaceholder: "name@company.com", shown: "shown", email: "Account email", type: "Type", discount: "Discount", attributed: "Joined", topups: "Top-ups", businessTerms: "B2B terms", makeB2b: "Make B2B", requestB2b: "Request B2B", editRates: "Edit rates", requestRates: "Request rates", noReferrals: "No referred accounts yet.", noSearchResults: "No accounts match this search.", unknownEmail: "Email unavailable",
    teamTitle: "Your Team", teamSub: "Invite an existing account by email. Choose your retained share and the permissions this member receives.", invite: "Invite a partner", inviting: "Sending…", retainedShare: "Your retained share", retainedHelp: "The part of this member’s platform commission that goes to you. Your maximum is {max}%.", memberRate: "Member commission", platformControlled: "10% by default · set by apiToken.sale", delegatedTeamLimit: "Their Team limit", delegatedTeamHelp: "Maximum share they may retain from their own members.", allowInvites: "Can build a Team", allowInvitesHelp: "May invite existing apiToken.sale accounts by email.", allowB2b: "Can set B2B terms", allowB2bHelp: "May convert their referrals and set a discount within the limit.", maxB2b: "Their B2B limit", allowB2bDelegate: "Can pass on B2B access", allowB2bDelegateHelp: "May give a smaller B2B limit to their own Team.", sendInvitation: "Send invitation", inviteSent: "Invitation sent.", existingOnly: "The email must belong to an active apiToken.sale account.", teamLimit: "Your Team limit", hardLimit: "Platform hard maximum 20%", directMembers: "Direct members", valid30: "Valid for 30 days",
    activeMembers: "Active members", pendingInvites: "Pending invitations", referralsCount: "Referrals", memberNet: "Member net", myShare: "Retained earnings", authority: "Permissions", edit: "Edit", save: "Save", saving: "Saving…", cancel: "Cancel", revoke: "Revoke", revokeInviteTitle: "Revoke invitation?", revokeInviteBody: "This permanently closes the pending invitation for {email}. The account can be invited again later.", confirmRevoke: "Revoke invitation", noTeam: "No Team members yet.", noInvites: "No pending invitations.", inviteExpires: "Expires", updateSaved: "Team settings saved.",
    requestsTitle: "Partner requests", requestsSub: "Ask apiToken.sale to review your platform commission. B2B requests are created from the Referrals table.", commissionRequest: "Request a higher commission", currentCommission: "Current commission", requestedCommission: "Requested commission", reason: "Why should it change?", reasonPlaceholder: "Describe your volume, active pipeline and expected growth…", sendRequest: "Send request", requestSent: "Request sent for review.",
    b2bTitle: "B2B referral pricing", b2bBody: "Use direct pricing only when the platform granted that permission. Otherwise submit the same customer and terms for review.", customerEmail: "Referral account email", requestType: "Request type", conversion: "Convert to B2B", pricing: "Change B2B pricing", requestedDiscount: "Requested discount", requestReview: "Request review", applyDirectly: "Apply directly", pricingApplied: "B2B pricing applied.", ceiling: "Your ceiling", noDirectB2b: "Direct B2B pricing is not enabled for this account.",
    requestHistory: "Request history", requestHistorySub: "Decisions and execution status are shown without internal account identifiers.", request: "Request", customer: "Customer", status: "Status", created: "Created", noRequests: "No requests yet.",
    payoutTitle: "Payouts", payoutSub: "Automatic twice-monthly payouts in USDT (BEP-20) on BNB Smart Chain.", wallet: "Payout wallet", walletHelp: "USDT (BEP-20) on BNB Smart Chain is the only supported network. Check the address carefully: on-chain payouts cannot be reversed.", saveWallet: "Save wallet", walletSaved: "Wallet saved.", changeWallet: "Change wallet", currentPeriod: "This period", nextPayout: "Next payout", availableToPay: "Available to pay", locked: "Locked", lifetimeNet: "Lifetime net", lifetimePaid: "Paid to date", debt: "Debt after refunds", minimum: "Minimum payout", lockedPeriods: "Locked periods", unlocks: "Unlocks", periodHistory: "Period history", phase: "Phase", payoutDate: "Payout date", noPayouts: "No payments yet.", payments: "Payments", paymentsSub: "On-chain USDT (BEP-20) transfers to your wallet.", amount: "Amount", method: "Method", tx: "Transaction", payoutRoadmap: "Payout roadmap", accruing: "Accruing", lock7: "7-day lock", pay3: "3-day payout", now: "Now", payoutHowTitle: "How payouts work", payoutHow: "Earnings accrue in two periods each month: days 1–15 and day 16 through month-end. After each period there is a 7-day lock, then a 3-day payout window. There is no manual withdrawal; if a wallet is missing, the amount rolls forward.",
    docsTitle: "Partner documentation", docsSub: "The complete program rules, from account attribution to a completed payout.", docsEarn: "1. What you earn", docsEarnBody: "You earn your platform commission from eligible API usage paid with a referral’s real funds. Free platform credit never counts.", docsIdentity: "2. Referral identity", docsIdentityBody: "Referrals and Team members are existing apiToken.sale accounts. They are assigned and shown by the same login email used in the customer Dashboard.", docsFormula: "3. Commission calculation", docsFormulaBody: "Commission equals your rate multiplied by the referral’s eligible paid API usage after their own discount. Refunds reverse the commission funded by the refunded payment.", docsTeam: "4. Team retained share", docsTeamBody: "A Team member receives the platform commission set by apiToken.sale (10% by default). You may retain a percentage of that commission, up to your personal limit and never above the platform hard maximum of 20%. This is not an additional commission.", docsB2b: "5. B2B referrals", docsB2bBody: "Open a referral’s B2B action to request conversion or new pricing. If self-service is enabled for your account, you may apply terms directly, never above your personal discount limit.", docsWallet: "6. Wallet and currency", docsWalletBody: "Payouts use USDT (BEP-20) on BNB Smart Chain. Without a bound wallet, earnings remain in the account and roll into a later payout.", docsSchedule: "7. Payout schedule", docsScheduleBody: "Earnings are grouped into the 1st–15th and 16th–last-day periods. Each completed period is locked for 7 days and paid during the following 3-day window.", docsPrivacy: "8. Access and privacy", docsPrivacyBody: "Only approved partners can open this workspace. Referral and Team identities are limited to account email; internal Commerce identifiers are never exposed.",
    invalidEmail: "Enter a valid account email.", invalidShare: "The retained share must be within your allowed maximum.", invalidReason: "Add a clear business justification.", invalidCommission: "Enter a commission from 0% to 100%.", invalidDiscount: "Enter a whole discount within your allowed ceiling.", invalidWallet: "Enter a valid 0x BSC wallet address.", mutationError: "The change could not be saved.",
    overviewLead: "You earn {rate} of what your referred accounts actually spend on API usage — the amount charged after their own discount, and only the part paid with real money. Free platform credit is spent first and never counts.",
    reflinkTitle: "Your referral link", reflinkSub: "Anyone who registers on apitoken.sale through this link is attributed to you permanently.", copyLink: "Copy link", copiedLink: "Copied", referralCodeLabel: "Referral code",
    howTitle: "How your commission works", how1: "You earn {rate} of what your referrals actually spend on API usage — the amount charged after their own discount.", how2: "Free platform credit never counts: it is spent first, and only usage covered by their own money earns commission.", how3: "Example: a referral spends $100 of real money on the API and you earn {example}.", how4: "Commission accrues as they spend and is paid out in USDT (BEP-20) on BNB Smart Chain.", howSplit: "So far: {direct} direct and {team} retained from your Team.",
    formulaTitle: "Your reward per $1 of real-money, list-price usage", formulaDiscount: "discount", formulaBody: "The client pays (100 − their discount)% of the price. You get {rate} of that — only the part paid with real money; free platform credit never counts.", formulaExample: "Example: $100 of usage with a 50% discount → they pay $50, you earn {example}.",
    recentTitle: "Recent activity", recentSub: "The latest accounts attributed to you.", joinedWord: "joined", earnedForYou: "earned for you", noActivity: "No referrals yet", noActivityBody: "Share your link to start building your list.",
    referredAccounts: "Referred accounts", netEarnings: "Net earnings", grossWord: "gross", returnsWord: "returns", realSpend30: "paid usage · 30 days", pendingPayout: "pending payout",
    periodHistorySub: "What you earned in each half-month period and when it is paid out.",
    payoutHowList: "Earnings are counted in two periods each month: the 1st–15th and the 16th–last day. After a period closes there is a 7-day lock, then everything earned is sent to your wallet within the next 3 days.", payoutHowWallet: "No wallet bound yet? Nothing is lost — the balance rolls into the next payout.",
    walletUnbound: "Bind your BSC wallet to receive payouts. Without it, your balance rolls over to the next period.",
  },
  ru: {
    eyebrow: "Партнёрская программа", title: "Рефералы", subtitle: "Ваши клиенты, команда, заработок и выплаты — внутри того же аккаунта, которым вы пользуетесь для API.",
    loading: "Загружаем партнёрские данные…", loadError: "Партнёрские данные временно недоступны.", retry: "Повторить",
    ordinaryTitle: "Развивайтесь вместе с apiToken.sale", ordinarySubtitle: "Партнёрский доступ одобряется индивидуально. Ниже — стандартные условия.", ordinaryBody: "Доступ к партнёрской программе включается вручную. Стандартные условия начинаются с комиссии 10% от оплаченного использования привлечённых клиентов. Рефералы и участники команды — существующие аккаунты apiToken.sale, которые определяются по почте аккаунта.",
    ordinaryPoint1: "Комиссия только с оплаченного использования", ordinaryPoint2: "Разбивка заработка по провайдерам и прозрачные периоды выплат", ordinaryPoint3: "Командная иерархия с удерживаемой долей максимум 20%",
    requestAccess: "Запросить доступ партнёра", contactHint: "Кнопка откроет диалог с @bozinodev в Telegram. Единственной учётной записью программы остаётся ваш аккаунт Dashboard.",
    disabledTitle: "Партнёрский доступ приостановлен", disabledBody: "Ваш аккаунт и история сохранены, но партнёрские действия отключены. Напишите в поддержку, чтобы уточнить причину или восстановить доступ.", contact: "Написать @bozinodev",
    overview: "Обзор", referrals: "Рефералы", team: "Команда", requests: "Заявки", payouts: "Выплаты", docs: "Документация",
    available: "Доступно", earned30: "Чистый доход · 30 дней", direct: "Прямой доход", teamIncome: "Удержано с команды", payable: "К выплате", fixedRate: "Ваша комиссия от платформы", fixedRateHint: "Устанавливает только apiToken.sale",
    chartTitle: "Заработок по дням", chartWindow: "Последние 30 дней", noEarnings: "В этом периоде партнёрского заработка пока нет.", providerSummary: "Итоги периода", providerCards: "Заработок по провайдерам", providerCardsSub: "То же представление, что в Usage, но только по оплаченному использованию рефералов.", ready: "Активен", events: "событий", earned: "Заработано", spend: "Оплачено клиентами", adjustments: "Корректировки", net: "Чистыми", dailyAverage: "В среднем за день", peakDay: "Лучший день",
    programTerms: "Как работает удержание с команды", termsBody: "Вы удерживаете Team-долю из фиксированной комиссии участника, а не получаете надбавку сверху. При $100 оплаченного расхода и комиссии 10% общий пул равен $10. Удержание 20% оставит участнику $8 и даст родителю $2 — общая выплата останется $10.",
    referralList: "Привлечённые аккаунты", referralListSub: "Аккаунты определяются по актуальной почте входа в apiToken.sale. Бесплатные средства платформы не входят в оплаченные траты.", searchReferrals: "Поиск по почте", searchPlaceholder: "name@company.com", shown: "показано", email: "Почта аккаунта", type: "Тип", discount: "Скидка", attributed: "С нами с", topups: "Пополнения", businessTerms: "B2B-условия", makeB2b: "Сделать B2B", requestB2b: "Запросить B2B", editRates: "Изменить ставки", requestRates: "Запросить ставки", noReferrals: "Привлечённых аккаунтов пока нет.", noSearchResults: "По этому запросу ничего не найдено.", unknownEmail: "Почта недоступна",
    teamTitle: "Ваша команда", teamSub: "Пригласите существующий аккаунт по почте. Выберите свою удерживаемую долю и доступные участнику права.", invite: "Пригласить партнёра", inviting: "Отправляем…", retainedShare: "Ваша доля", retainedHelp: "Часть комиссии участника, которая остаётся вам. Ваш максимум — {max}%.", memberRate: "Комиссия участника", platformControlled: "По умолчанию 10% · задаёт apiToken.sale", delegatedTeamLimit: "Его лимит команды", delegatedTeamHelp: "Максимальная доля, которую он сможет удерживать со своей команды.", allowInvites: "Может собирать команду", allowInvitesHelp: "Сможет приглашать существующие аккаунты apiToken.sale по почте.", allowB2b: "Может назначать B2B", allowB2bHelp: "Сможет переводить своих рефералов в B2B и назначать скидку в пределах лимита.", maxB2b: "Его B2B-лимит", allowB2bDelegate: "Может передавать право B2B", allowB2bDelegateHelp: "Сможет дать своей команде меньший B2B-лимит.", sendInvitation: "Отправить приглашение", inviteSent: "Приглашение отправлено.", existingOnly: "Почта должна принадлежать активному аккаунту apiToken.sale.", teamLimit: "Ваш лимит команды", hardLimit: "Глобальный максимум 20%", directMembers: "Прямые участники", valid30: "Действуют 30 дней",
    activeMembers: "Участники", pendingInvites: "Ожидающие приглашения", referralsCount: "Рефералы", memberNet: "Доход участника", myShare: "Удержано вами", authority: "Полномочия", edit: "Изменить", save: "Сохранить", saving: "Сохраняем…", cancel: "Отмена", revoke: "Отозвать", revokeInviteTitle: "Отозвать приглашение?", revokeInviteBody: "Ожидающее приглашение для {email} будет закрыто. Позже аккаунт можно будет пригласить снова.", confirmRevoke: "Отозвать приглашение", noTeam: "В команде пока никого нет.", noInvites: "Ожидающих приглашений нет.", inviteExpires: "Истекает", updateSaved: "Настройки участника сохранены.",
    requestsTitle: "Партнёрские заявки", requestsSub: "Запросите пересмотр своей комиссии. Заявки на B2B создаются из таблицы «Рефералы».", commissionRequest: "Запросить повышение комиссии", currentCommission: "Текущая комиссия", requestedCommission: "Желаемая комиссия", reason: "Почему её нужно изменить?", reasonPlaceholder: "Опишите объём, активную воронку и ожидаемый рост…", sendRequest: "Отправить заявку", requestSent: "Заявка отправлена на рассмотрение.",
    b2bTitle: "B2B-условия реферала", b2bBody: "Назначайте условия напрямую только при выданном платформой разрешении. Иначе отправьте те же данные на рассмотрение.", customerEmail: "Почта аккаунта реферала", requestType: "Тип заявки", conversion: "Перевести в B2B", pricing: "Изменить B2B-условия", requestedDiscount: "Запрашиваемая скидка", requestReview: "Запросить согласование", applyDirectly: "Применить напрямую", pricingApplied: "B2B-условия применены.", ceiling: "Ваш максимум", noDirectB2b: "Самостоятельное назначение B2B-условий для этого аккаунта не включено.",
    requestHistory: "История заявок", requestHistorySub: "Решения и исполнение показаны без внутренних идентификаторов аккаунтов.", request: "Заявка", customer: "Клиент", status: "Статус", created: "Создана", noRequests: "Заявок пока нет.",
    payoutTitle: "Выплаты", payoutSub: "Автоматические выплаты дважды в месяц в USDT (BEP-20) в сети BNB Smart Chain.", wallet: "Кошелёк для выплат", walletHelp: "Поддерживается только USDT (BEP-20) в сети BNB Smart Chain. Проверьте адрес: выплату в блокчейне нельзя отменить.", saveWallet: "Сохранить кошелёк", walletSaved: "Кошелёк сохранён.", changeWallet: "Изменить кошелёк", currentPeriod: "Текущий период", nextPayout: "Следующая выплата", availableToPay: "Доступно к выплате", locked: "Заблокировано", lifetimeNet: "За всё время", lifetimePaid: "Выплачено", debt: "Долг после возвратов", minimum: "Минимальная выплата", lockedPeriods: "Заблокированные периоды", unlocks: "Разблокировка", periodHistory: "История периодов", phase: "Этап", payoutDate: "Дата выплаты", noPayouts: "Выплат пока нет.", payments: "Выплаты", paymentsSub: "On-chain переводы USDT (BEP-20) на ваш кошелёк.", amount: "Сумма", method: "Метод", tx: "Транзакция", payoutRoadmap: "Карта выплат", accruing: "Начисление", lock7: "Лок 7 дней", pay3: "Выплата 3 дня", now: "Сейчас", payoutHowTitle: "Как проходят выплаты", payoutHow: "Доход считается по двум периодам: с 1-го по 15-е и с 16-го по последний день месяца. Затем действует лок 7 дней и окно выплаты 3 дня. Ручного вывода нет; без кошелька сумма переносится дальше.",
    docsTitle: "Документация партнёра", docsSub: "Полные правила программы: от привязки аккаунта до завершённой выплаты.", docsEarn: "1. За что вы зарабатываете", docsEarnBody: "Вы получаете свою комиссию от использования API, оплаченного реальными средствами реферала. Бесплатные средства платформы не учитываются.", docsIdentity: "2. Как определяются рефералы", docsIdentityBody: "Рефералы и участники команды — существующие аккаунты apiToken.sale. Они назначаются и отображаются по той же почте, с которой входят в клиентский Dashboard.", docsFormula: "3. Как считается комиссия", docsFormulaBody: "Комиссия равна вашей ставке, умноженной на оплаченные траты реферала после его скидки. Возврат платежа отменяет начисленную с него комиссию.", docsTeam: "4. Удержание с команды", docsTeamBody: "Участник получает комиссию от apiToken.sale — по умолчанию 10%. Вы можете удерживать часть этой комиссии в пределах личного лимита, но не больше глобальных 20%. Это не дополнительная комиссия.", docsB2b: "5. B2B-рефералы", docsB2bBody: "Откройте B2B-действие у конкретного реферала, чтобы запросить перевод или новые условия. Если вам доступно самостоятельное управление, условия можно применить сразу, но не выше личного лимита скидки.", docsWallet: "6. Кошелёк и валюта", docsWalletBody: "Выплаты отправляются в USDT (BEP-20) по сети BNB Smart Chain. Если кошелёк не привязан, заработок сохраняется и переносится на следующую выплату.", docsSchedule: "7. График выплат", docsScheduleBody: "Доход группируется по периодам 1–15 и 16–последний день месяца. Завершённый период блокируется на 7 дней и выплачивается в следующие 3 дня.", docsPrivacy: "8. Доступ и приватность", docsPrivacyBody: "Раздел доступен только одобренным партнёрам. Рефералы и команда показываются по почте аккаунта; внутренние идентификаторы Commerce не раскрываются.",
    invalidEmail: "Введите корректную почту аккаунта.", invalidShare: "Удерживаемая доля должна быть в пределах доступного максимума.", invalidReason: "Добавьте понятное обоснование.", invalidCommission: "Введите комиссию от 0% до 100%.", invalidDiscount: "Введите целую скидку в пределах доступного максимума.", invalidWallet: "Введите корректный адрес BSC-кошелька, начинающийся с 0x.", mutationError: "Не удалось сохранить изменение.",
    overviewLead: "Вы получаете {rate} от того, что привлечённые аккаунты реально тратят на использование API — от суммы после их собственной скидки и только с части, оплаченной настоящими деньгами. Бесплатные средства платформы тратятся первыми и не считаются.",
    reflinkTitle: "Ваша реферальная ссылка", reflinkSub: "Каждый, кто зарегистрируется на apitoken.sale по этой ссылке, навсегда закрепляется за вами.", copyLink: "Копировать ссылку", copiedLink: "Скопировано", referralCodeLabel: "Реферальный код",
    howTitle: "Как работает ваша комиссия", how1: "Вы получаете {rate} от того, что рефералы реально тратят на использование API — от суммы после их собственной скидки.", how2: "Бесплатные средства платформы не считаются: они тратятся первыми, и комиссию приносит только использование, оплаченное деньгами клиента.", how3: "Пример: реферал тратит $100 реальных денег на API, и вы получаете {example}.", how4: "Комиссия начисляется по мере трат и выплачивается в USDT (BEP-20) в сети BNB Smart Chain.", howSplit: "Сейчас: {direct} напрямую и {team} удержано с команды.",
    formulaTitle: "Ваша награда с $1 реального использования по прайсу", formulaDiscount: "скидка", formulaBody: "Клиент платит (100 − его скидка)% от цены. Вы получаете {rate} от этой суммы — только с части, оплаченной реальными деньгами; бесплатные средства не считаются.", formulaExample: "Пример: $100 использования со скидкой 50% → клиент платит $50, вы получаете {example}.",
    recentTitle: "Недавняя активность", recentSub: "Последние аккаунты, закреплённые за вами.", joinedWord: "присоединился", earnedForYou: "заработано для вас", noActivity: "Пока нет рефералов", noActivityBody: "Поделитесь ссылкой, чтобы начать собирать базу.",
    referredAccounts: "Привлечённые аккаунты", netEarnings: "Чистый заработок", grossWord: "начислено", returnsWord: "возвраты", realSpend30: "оплаченного расхода · 30 дней", pendingPayout: "ожидает выплаты",
    periodHistorySub: "Сколько вы заработали в каждом полумесячном периоде и когда это выплачивается.",
    payoutHowList: "Заработок считается по двум периодам каждый месяц: с 1-го по 15-е и с 16-го по последний день. После закрытия периода действует лок 7 дней, затем весь заработок уходит на ваш кошелёк в течение следующих 3 дней.", payoutHowWallet: "Кошелёк ещё не привязан? Ничего не теряется — баланс переносится на следующую выплату.",
    walletUnbound: "Привяжите кошелёк BSC, чтобы получать выплаты. Без него баланс переносится на следующий период.",
  },
} as const;

function tabFromUrl(): ReferralTab {
  if (typeof window === "undefined") return "overview";
  const value = new URLSearchParams(window.location.search).get("tab");
  return TABS.includes(value as ReferralTab) ? value as ReferralTab : "overview";
}

function pct(bps: number, locale: string): string {
  return `${(bps / 100).toLocaleString(locale, { maximumFractionDigits: 2 })}%`;
}

function date(value: string | null, locale: string): string {
  if (!value) return "—";
  const parsed = new Date(value);
  return Number.isNaN(parsed.valueOf()) ? "—" : new Intl.DateTimeFormat(locale, { dateStyle: "medium" }).format(parsed);
}

function validEmail(value: string): boolean {
  return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value.trim()) && value.length <= 320;
}

function mutationKey(): string {
  return typeof crypto.randomUUID === "function" ? crypto.randomUUID() : `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

function errorMessage(cause: unknown, fallback: string): string {
  if (cause instanceof ApiError && cause.status < 500 && cause.message) return cause.message;
  return fallback;
}

/** Commission on $100 of real-money usage, in nanoUSD — integers only, never float money. */
function commissionOnHundred(commissionBps: number): bigint {
  return BigInt(commissionBps) * 100n * 1_000_000_000n / 10_000n;
}

export function Referral() {
  const { language } = useI18n();
  const text = copy[language];
  const [snapshot, setSnapshot] = useState<ReferralSnapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState(false);
  const [tab, setTab] = useState<ReferralTab>(tabFromUrl);

  const load = useCallback(async (quiet = false) => {
    if (!quiet) setLoading(true);
    setLoadError(false);
    try { setSnapshot(await api.referral()); }
    catch { setLoadError(true); }
    finally { if (!quiet) setLoading(false); }
  }, []);

  useEffect(() => {
    let active = true;
    void api.referral()
      .then((next) => { if (active) setSnapshot(next); })
      .catch(() => { if (active) setLoadError(true); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, []);
  useEffect(() => {
    const sync = () => setTab(tabFromUrl());
    window.addEventListener("popstate", sync);
    return () => window.removeEventListener("popstate", sync);
  }, []);

  function selectTab(next: ReferralTab) {
    setTab(next);
    const url = new URL(window.location.href);
    url.searchParams.set("view", "referral");
    if (next === "overview") url.searchParams.delete("tab");
    else url.searchParams.set("tab", next);
    window.history.pushState(null, "", `${url.pathname}${url.search}${url.hash}`);
  }

  if (loading) return <section className="panel referral-panel rp"><PageHeading eyebrow={text.eyebrow} title={text.title} subtitle={text.subtitle} /><div className="referral-loading" role="status"><span className="spinner" />{text.loading}</div></section>;
  if (loadError || !snapshot) return <section className="panel referral-panel rp"><PageHeading eyebrow={text.eyebrow} title={text.title} subtitle={text.subtitle} /><div className="empty-box" role="alert"><p>{text.loadError}</p><button type="button" className="btn btn-ghost" onClick={() => void load()}>{text.retry}</button></div></section>;
  if (snapshot.state === "unavailable") return <OrdinaryState language={language} />;
  if (snapshot.state === "disabled") return <DisabledState language={language} />;

  return <section className="panel referral-panel rp">
    <PageHeading eyebrow={text.eyebrow} title={text.title} subtitle={text.subtitle} />
    <nav className="referral-subnav" aria-label={text.title}>
      {TABS.map((item) => <button key={item} type="button" className={item === tab ? "on" : ""} aria-current={item === tab ? "page" : undefined} onClick={() => selectTab(item)}>{text[item]}</button>)}
    </nav>
    {tab === "overview" && <PartnerOverview snapshot={snapshot} language={language} />}
    {tab === "referrals" && <ReferralAccounts snapshot={snapshot} language={language} />}
    {tab === "team" && <Team snapshot={snapshot} language={language} refresh={() => load(true)} />}
    {tab === "payouts" && <Payouts snapshot={snapshot} language={language} refresh={() => load(true)} />}
    {tab === "docs" && <PartnerDocs snapshot={snapshot} language={language} />}
  </section>;
}

function OrdinaryState({ language }: { language: Language }) {
  const text = copy[language];
  return <section className="panel referral-panel rp"><PageHeading eyebrow={text.eyebrow} title={text.title} subtitle={text.ordinarySubtitle} />
    <div className="referral-access-card card">
      <div><span className="overview-status setup"><i />{text.eyebrow}</span><h2>{text.ordinaryTitle}</h2><p>{text.ordinaryBody}</p></div>
      <ul><li>{text.ordinaryPoint1}</li><li>{text.ordinaryPoint2}</li><li>{text.ordinaryPoint3}</li></ul>
      <a className="btn btn-primary" href="https://t.me/bozinodev" target="_blank" rel="noreferrer">{text.requestAccess}</a>
      <small>{text.contactHint}</small>
    </div>
  </section>;
}

function DisabledState({ language }: { language: Language }) {
  const text = copy[language];
  return <section className="panel referral-panel rp"><PageHeading eyebrow={text.eyebrow} title={text.title} subtitle={text.subtitle} />
    <div className="referral-access-card card referral-disabled-card"><div><span className="overview-status warning"><i />{text.disabledTitle}</span><h2>{text.disabledTitle}</h2><p>{text.disabledBody}</p></div><a className="btn btn-ghost" href="https://t.me/bozinodev" target="_blank" rel="noreferrer">{text.contact}</a></div>
  </section>;
}

// ---------------------------------------------------------------------------
// Partner-cabinet primitives (the partners.apitoken.sale visual contract)
// ---------------------------------------------------------------------------

function PageTitle({ title, sub }: { title: string; sub: ReactNode }) {
  return <><h2 className="rp-title">{title}</h2><p className="rp-sub">{sub}</p></>;
}

function Card({ title, sub, children, className }: { title?: string; sub?: ReactNode; children?: ReactNode; className?: string }) {
  return <section className={`rp-card${className ? ` ${className}` : ""}`}>
    {title && <h3 className="rp-card-title">{title}</h3>}
    {sub && <p className="rp-card-sub">{sub}</p>}
    {children}
  </section>;
}

function StatStrip({ items }: { items: Array<{ label: string; value: string; foot: string; accent?: boolean }> }) {
  return <div className="rp-stats">
    {items.map((item) => <div className="rp-stat" key={item.label}>
      <div className="rp-stat-l">{item.label}</div>
      <div className={`rp-stat-v${item.accent ? " accent" : ""}`}>{item.value}</div>
      <div className="rp-stat-f">{item.foot}</div>
    </div>)}
  </div>;
}

function Table({ label, head, children, className }: { label: string; head: ReactNode; children: ReactNode; className?: string }) {
  return <div className="rp-table-wrap" role="region" aria-label={label} tabIndex={0}>
    <table className={`rp-table${className ? ` ${className}` : ""}`}><thead><tr>{head}</tr></thead><tbody>{children}</tbody></table>
  </div>;
}

function EmptyRow({ span, title, body }: { span: number; title: string; body?: string }) {
  return <tr><td colSpan={span}><div className="rp-empty"><b>{title}</b>{body}</div></td></tr>;
}

// ---------------------------------------------------------------------------
// Overview — the partner cabinet landing page
// ---------------------------------------------------------------------------

function PartnerOverview({ snapshot, language }: { snapshot: ReferralActiveSnapshot; language: Language }) {
  const text = copy[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const rate = pct(snapshot.membership.commissionBps, locale);
  const example = formatNanoUsd(commissionOnHundred(snapshot.membership.commissionBps), locale);
  const referralUrl = `${PARTNER_SITE_ORIGIN}/?ref=${snapshot.membership.referralCode}`;
  const recent = [...snapshot.referrals].sort((left, right) => left.attributedAt < right.attributedAt ? 1 : -1).slice(0, 6);
  const debt = BigInt(snapshot.totals.debtNano);

  return <div className="referral-tab-panel">
    <PageTitle title={text.overview} sub={interpolate(text.overviewLead, { rate })} />
    <StatStrip items={[
      { label: text.available, value: formatNanoUsd(snapshot.totals.availableNano, locale), foot: `${formatNanoUsd(snapshot.totals.pendingPayoutNano, locale)} ${text.pendingPayout}`, accent: true },
      { label: text.netEarnings, value: formatNanoUsd(snapshot.totals.netNano, locale), foot: `${formatNanoUsd(snapshot.totals.earnedNano, locale)} ${text.grossWord} · ${formatNanoUsd(snapshot.totals.adjustmentNano, locale)} ${text.returnsWord}` },
      { label: text.referredAccounts, value: snapshot.referrals.length.toLocaleString(locale), foot: `${formatNanoUsd(snapshot.totals.last30dSpendNano, locale)} ${text.realSpend30}` },
      { label: text.earned30, value: formatNanoUsd(snapshot.totals.last30dNetNano, locale), foot: `${text.adjustments}: ${formatNanoUsd(snapshot.totals.last30dAdjustmentNano, locale)}` },
    ]} />

    {debt > 0n && <div className="rp-note bad" style={{ marginTop: 24 }} role="alert"><strong>{text.debt}: {formatNanoUsd(snapshot.totals.debtNano, locale)}.</strong>{" "}{language === "ru" ? "Будущие начисления сначала погасят долг; внешний кошелёк автоматически не списывается." : "Future earnings repay it first; the external wallet is never debited automatically."}</div>}

    <div className="rp-stack" style={{ marginTop: 24 }}>
      <CommissionFormula snapshot={snapshot} language={language} />

      <Card title={text.reflinkTitle} sub={text.reflinkSub}>
        <div className="rp-reflink">
          <input id="partner-referral-url" readOnly translate="no" value={referralUrl} aria-label={text.reflinkTitle} onFocus={(event) => event.currentTarget.select()} />
          <CopyButton value={referralUrl} label={text.copyLink} copiedLabel={text.copiedLink} />
        </div>
        <p className="rp-code">{text.referralCodeLabel}: <b translate="no">{snapshot.membership.referralCode}</b></p>
      </Card>

      <Card title={text.howTitle}>
        <ul className="rp-how">
          <li>{interpolate(text.how1, { rate })}</li>
          <li>{text.how2}</li>
          <li>{interpolate(text.how3, { example })}</li>
          <li>{text.how4}</li>
          <li>{interpolate(text.howSplit, { direct: formatNanoUsd(snapshot.totals.directNetNano, locale), team: formatNanoUsd(snapshot.totals.overrideNetNano, locale) })}</li>
        </ul>
      </Card>

      <Card title={text.providerCards} sub={text.providerCardsSub}>
        <ProviderCards snapshot={snapshot} language={language} />
      </Card>

      <Card title={text.chartTitle} sub={text.chartWindow}>
        <EarningsChart snapshot={snapshot} language={language} />
      </Card>

      <Card title={text.recentTitle} sub={text.recentSub}>
        {recent.length === 0 ? <div className="rp-empty"><b>{text.noActivity}</b>{text.noActivityBody}</div> : <div className="rp-activity">
          {recent.map((item, index) => <div className="rp-activity-row" key={`${item.email ?? "unknown"}-${index}`}>
            <span><span className="referral-email" translate="no">{item.email ?? text.unknownEmail}</span> {text.joinedWord} · <b>{formatNanoUsd(item.netNano, locale)}</b> {text.earnedForYou}</span>
            <span className="rp-activity-date">{date(item.attributedAt, locale)}</span>
          </div>)}
        </div>}
      </Card>
    </div>
  </div>;
}

function CommissionFormula({ snapshot, language }: { snapshot: ReferralActiveSnapshot; language: Language }) {
  const text = copy[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const rate = pct(snapshot.membership.commissionBps, locale);
  // Half price after a 50% client discount: the partner earns their rate on $50.
  const example = formatNanoUsd(commissionOnHundred(snapshot.membership.commissionBps) / 2n, locale);
  return <div className="rp-formula">
    <div className="rp-formula-l">{text.formulaTitle}</div>
    <div className="rp-formula-v">(100% − <em>{text.formulaDiscount}</em>%)<i>×</i><em>{rate}</em></div>
    <p className="rp-formula-b">{interpolate(text.formulaBody, { rate })}</p>
    <p className="rp-formula-x">{interpolate(text.formulaExample, { example })}</p>
  </div>;
}

const HIDDEN_REFERRAL_PROVIDERS = new Set(["glm", "zai", "zhipu"]);

function ProviderCards({ snapshot, language }: { snapshot: ReferralActiveSnapshot; language: Language }) {
  const text = copy[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const registry = new Map(DASHBOARD_PROVIDERS.map((provider) => [provider.id, provider]));
  const rows = new Map(snapshot.earnings.providers
    .filter((row) => !row.providerId || !HIDDEN_REFERRAL_PROVIDERS.has(row.providerId))
    .map((row) => [row.providerId ?? "unattributed", row]));
  const ids = [...DASHBOARD_PROVIDERS.map((provider) => provider.id)];
  for (const id of rows.keys()) if (!ids.includes(id)) ids.push(id);
  const total = [...rows.values()].reduce((sum, row) => sum + BigInt(row.earnedNano), 0n);
  const metadata = (id: string) => registry.get(id) ?? fallbackProvider(id, id === "unattributed" ? (language === "ru" ? "Без провайдера" : "Unattributed") : id);

  return <div className="uprovider-grid">
    {ids.map((id) => {
      const provider = metadata(id);
      const row = rows.get(id);
      const earned = BigInt(row?.earnedNano ?? "0");
      const shareTenths = total > 0n ? Number(earned * 1_000n / total) : 0;
      return <article className="uprovider-card" key={id} style={{ "--provider-color": provider.color, ...(provider.logo ? { "--provider-logo": `url("${provider.logo}")` } : {}) } as CSSProperties}>
        <div className="uprovider-head">
          {provider.logo ? <span className="uprovider-logo" aria-hidden="true" /> : <span className="uprovider-logo uprovider-letter" aria-hidden="true">{provider.name.slice(0, 1)}</span>}
          <div className="uprovider-name"><strong>{provider.name}</strong><span>{provider.api}</span></div>
          <span className="uprovider-status is-active">{text.ready}</span>
          <span className="uprovider-discount">{(shareTenths / 10).toLocaleString(locale, { minimumFractionDigits: 1, maximumFractionDigits: 1 })}%</span>
        </div>
        <div className="uprovider-stats"><strong>{formatNanoUsd(earned, locale)}</strong><span>{formatNanoUsd(row?.spendNano ?? "0", locale)} {text.spend.toLocaleLowerCase()} · {(row?.events ?? 0).toLocaleString(locale)} {text.events}</span></div>
      </article>;
    })}
  </div>;
}

function EarningsChart({ snapshot, language }: { snapshot: ReferralActiveSnapshot; language: Language }) {
  const text = copy[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const registry = new Map(DASHBOARD_PROVIDERS.map((provider) => [provider.id, provider]));
  const metadata = (id: string) => registry.get(id) ?? fallbackProvider(id, id === "unattributed" ? (language === "ru" ? "Без провайдера" : "Unattributed") : id);
  const points = snapshot.earnings.providerDaily.map((point) => ({
    date: point.date,
    providers: point.providers
      .filter((provider) => !provider.providerId || !HIDDEN_REFERRAL_PROVIDERS.has(provider.providerId))
      .map((provider) => ({ id: provider.providerId ?? "unattributed", events: provider.events, spend: BigInt(provider.spendNano), earned: BigInt(provider.earnedNano) })),
  }));
  const ids = [...new Set(points.flatMap((point) => point.providers.filter((provider) => provider.earned > 0n).map((provider) => provider.id)))];
  const providerOrder = new Map(DASHBOARD_PROVIDERS.map((provider, index) => [provider.id, index]));
  ids.sort((left, right) => (providerOrder.get(left) ?? 999) - (providerOrder.get(right) ?? 999) || left.localeCompare(right));
  const providers = ids.map(metadata);
  const totals = points.map((point) => point.providers.reduce((sum, provider) => sum + provider.earned, 0n));
  const rawMax = totals.reduce((value, item) => item > value ? item : value, 0n);
  const scale = niceReferralScale(rawMax);
  const gridTicks = Array.from({ length: scale.divisions + 1 }, (_, index) => scale.max - BigInt(index) * scale.step);
  const [hover, setHover] = useState<number | null>(null);
  const markCount = Math.min(7, points.length);
  const marks = points.length === 0 ? [] : [...new Set(Array.from({ length: markCount }, (_, index) => Math.round(index * (points.length - 1) / Math.max(1, markCount - 1))))];
  const totalEarned = totals.reduce((sum, value) => sum + value, 0n);
  const totalSpend = points.reduce((sum, point) => sum + point.providers.reduce((day, provider) => day + provider.spend, 0n), 0n);
  const totalEvents = points.reduce((sum, point) => sum + point.providers.reduce((day, provider) => day + provider.events, 0), 0);
  const peakIndex = totals.reduce((best, value, index) => value > (totals[best] ?? 0n) ? index : best, 0);

  return <div className="usage-graph referral-earnings-graph">
    <div className="uchart">
      <div className="uchart-head"><div className="uchart-head-meta"><div className="usage-chart-legend" aria-label={text.providerSummary}>{providers.map((provider) => <span key={provider.id}><i style={{ background: provider.color }} />{provider.name}</span>)}</div></div></div>
      {rawMax === 0n ? <div className="uchart-empty">{text.noEarnings}</div> : <div className="uchart-grid">
        <div className="uchart-yaxis">{gridTicks.map((tick, index) => <span key={index}>{formatReferralAxis(tick, locale)}</span>)}</div>
        <div className="uchart-plotwrap"><div className="uchart-lines">{gridTicks.map((_, index) => <i key={index} />)}</div>
          <div className="uchart-plot" onMouseLeave={(event) => { if (!event.currentTarget.contains(document.activeElement)) setHover(null); }}>
            {points.map((point, index) => <button type="button" key={`${point.date}-${index}`} className={`uchart-col${hover === index ? " is-hover" : ""}`} aria-label={[`${date(point.date, locale)}. ${text.earned}: ${formatNanoUsd(totals[index] ?? 0n, locale)}`, ...point.providers.filter((item) => item.earned > 0n).map((item) => `${metadata(item.id).name}: ${formatNanoUsd(item.earned, locale)}`)].join(". ")} onMouseEnter={() => setHover(index)} onFocus={() => setHover(index)} onBlur={() => setHover((current) => current === index ? null : current)} onClick={() => setHover((current) => current === index ? null : index)} onKeyDown={(event) => { if (event.key === "Escape") { setHover(null); event.currentTarget.blur(); } }}><div className="uchart-col-fill">{providers.map((provider) => { const item = point.providers.find((candidate) => candidate.id === provider.id); return item && item.earned > 0n ? <div className="uchart-seg" key={provider.id} style={{ height: `${boundedReferralPercent(item.earned, scale.max)}%`, background: provider.color }} /> : null; })}</div></button>)}
            {hover !== null && points[hover] && (totals[hover] ?? 0n) > 0n && <div className="chart-tip" role="tooltip" style={{ left: `${Math.min(92, Math.max(8, (hover + .5) / points.length * 100))}%`, bottom: `${boundedReferralPercent(totals[hover] ?? 0n, scale.max)}%` }}><div className="chart-tip-h">{date(points[hover]!.date, locale)}</div>{providers.map((provider) => { const item = points[hover]!.providers.find((candidate) => candidate.id === provider.id); return item && item.earned > 0n ? <div className="chart-tip-row" key={provider.id}><span className="chart-tip-dot" style={{ background: provider.color }} /><span className="chart-tip-nm">{provider.name}</span><b>{formatNanoUsd(item.earned, locale)}</b></div> : null; })}<div className="chart-tip-total"><span>{text.earned}</span><b>{formatNanoUsd(totals[hover] ?? 0n, locale)}</b></div></div>}
          </div>
          <div className="uchart-axis">{marks.map((mark) => <span key={mark} style={{ left: `${(mark + .5) / points.length * 100}%` }}>{date(points[mark]!.date, locale)}</span>)}</div>
        </div>
      </div>}
    </div>
    <div className="usum"><span className="usum-t">{text.providerSummary}</span><div className="usum-row"><span>{text.earned}</span><b className="accent">{formatNanoUsd(totalEarned, locale)}</b></div><div className="usum-row"><span>{text.spend}</span><b>{formatNanoUsd(totalSpend, locale)}</b></div><div className="usum-row"><span>{text.events}</span><b>{totalEvents.toLocaleString(locale)}</b></div><div className="usum-row"><span>{text.peakDay}</span><b>{rawMax > 0n && points[peakIndex] ? `${date(points[peakIndex]!.date, locale)} · ${formatNanoUsd(rawMax, locale)}` : "—"}</b></div><div className="usum-row"><span>{text.dailyAverage}</span><b>{points.length ? formatNanoUsd(totalEarned / BigInt(points.length), locale) : "—"}</b></div></div>
  </div>;
}

function niceReferralScale(max: bigint): { max: bigint; step: bigint; divisions: number } {
  const divisions = 4;
  const dollar = 1_000_000_000n;
  if (max <= 0n) return { max: dollar, step: dollar / 4n, divisions };
  const rough = (max + BigInt(divisions) - 1n) / BigInt(divisions);
  const magnitude = 10n ** BigInt(Math.max(0, rough.toString().length - 1));
  const step = [magnitude, 2n * magnitude, 5n * magnitude, 10n * magnitude].find((candidate) => candidate >= rough) ?? 10n * magnitude;
  return { max: step * BigInt(divisions), step, divisions };
}

function formatReferralAxis(value: bigint, locale: string): string {
  if (value <= 0n) return "$0";
  if (value >= 1_000_000_000n) return formatNanoUsd(value, locale, 0, 1);
  if (value >= 10_000_000n) return formatNanoUsd(value, locale, 0, 2);
  return formatNanoUsd(value, locale, 0, 4);
}

function boundedReferralPercent(value: bigint, maximum: bigint): number {
  if (value <= 0n || maximum <= 0n) return 0;
  const bounded = value > maximum ? maximum : value;
  return Number(bounded * 1_000_000n / maximum) / 10_000;
}

// ---------------------------------------------------------------------------
// Referrals — the directory the user approved; only its heading follows the cabinet.
// ---------------------------------------------------------------------------

function ReferralAccounts({ snapshot, language }: { snapshot: ReferralActiveSnapshot; language: Language }) {
  const text = copy[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const [query, setQuery] = useState("");
  const [pricing, setPricing] = useState<ReferralActiveSnapshot["referrals"][number] | null>(null);
  const normalized = query.trim().toLocaleLowerCase(locale);
  const rows = normalized ? snapshot.referrals.filter((item) => (item.email ?? "").toLocaleLowerCase(locale).includes(normalized)) : snapshot.referrals;
  const direct = snapshot.membership.b2bEnabled && snapshot.membership.b2bMaxDiscountBps > 0;

  return <div className="referral-tab-panel">
    <PageTitle title={text.referralList} sub={text.referralListSub} />
    <div className="referral-directory-toolbar">
      <Field label={text.searchReferrals}><span className="referral-search"><svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="6.5" /><path d="m16 16 4 4" /></svg><input type="search" autoComplete="off" spellCheck={false} value={query} onChange={(event) => setQuery(event.target.value)} placeholder={text.searchPlaceholder} /></span></Field>
      <span>{rows.length.toLocaleString(locale)} / {snapshot.referrals.length.toLocaleString(locale)} {text.shown}</span>
      <span className="referral-ceiling-chip">{text.ceiling}: <b>{pct(snapshot.membership.b2bMaxDiscountBps, locale)}</b></span>
    </div>
    <p className="table-scroll-hint">{language === "ru" ? "Таблица прокручивается по горизонтали" : "Scroll the table horizontally"}</p>
    <div className="table-scroll" role="region" tabIndex={0} aria-label={text.referralList}><table className="mtable referral-table referral-directory-table"><thead><tr><th>{text.email}</th><th>{text.type}</th><th className="tnum">{text.discount}</th><th className="tnum">{text.topups}</th><th className="tnum">{text.spend}</th><th className="tnum">{text.earned}</th><th>{text.businessTerms}</th></tr></thead><tbody>{snapshot.referrals.length === 0 ? <tr><td colSpan={7} className="empty-cell">{text.noReferrals}</td></tr> : rows.length === 0 ? <tr><td colSpan={7} className="empty-cell">{text.noSearchResults}</td></tr> : rows.map((item, index) => <tr key={`${item.email ?? "unknown"}-${index}`}><td><span className="referral-email" translate="no">{item.email ?? text.unknownEmail}</span><small>{text.attributed}: {date(item.attributedAt, locale)}</small></td><td><Status value={item.customerType?.toUpperCase() ?? "—"} kind={item.customerType === "b2b" ? "ok" : undefined} /></td><td className="tnum">{item.discountBps === null ? "—" : pct(item.discountBps, locale)}</td><td className="tnum">{formatNanoUsd(item.topupNano, locale)}</td><td className="tnum">{formatNanoUsd(item.spendNano, locale)}</td><td className="tnum referral-positive">{formatNanoUsd(item.netNano, locale)}{BigInt(item.adjustmentNano) !== 0n && <small>{formatNanoUsd(item.adjustmentNano, locale)} {text.adjustments.toLocaleLowerCase()}</small>}</td><td>{item.email ? <button type="button" className="btn btn-ghost btn-sm" onClick={() => setPricing(item)}>{direct ? item.customerType === "b2b" ? text.editRates : text.makeB2b : item.customerType === "b2b" ? text.requestRates : text.requestB2b}</button> : "—"}</td></tr>)}</tbody></table></div>
    {pricing && <BusinessPricingDialog row={pricing} snapshot={snapshot} language={language} onClose={() => setPricing(null)} />}
  </div>;
}

function BusinessPricingDialog({ row, snapshot, language, onClose }: { row: ReferralActiveSnapshot["referrals"][number]; snapshot: ReferralActiveSnapshot; language: Language; onClose(): void }) {
  const text = copy[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const direct = snapshot.membership.b2bEnabled && snapshot.membership.b2bMaxDiscountBps > 0;
  const ceilingPercent = direct ? Math.floor(snapshot.membership.b2bMaxDiscountBps / 100) : 95;
  const [discount, setDiscount] = useState(row.discountBps === null ? "" : String(Math.floor(row.discountBps / 100)));
  const initialProviders = Object.fromEntries(DASHBOARD_PROVIDERS.map((provider) => [provider.id, row.providerDiscounts.find((item) => item.providerId === provider.id)?.discountBps == null ? "" : String(Math.floor(row.providerDiscounts.find((item) => item.providerId === provider.id)!.discountBps / 100))]));
  const [providers, setProviders] = useState<Record<string, string>>(initialProviders);
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const closeRef = useModalFocus(() => { if (!busy) onClose(); });

  function parsed(value: string): number | null {
    if (!/^\d{1,2}$/.test(value.trim())) return null;
    const number = Number(value);
    return Number.isInteger(number) && number >= 0 && number <= ceilingPercent ? number : null;
  }

  async function save() {
    setNotice(null);
    const base = parsed(discount);
    if (base === null) return setNotice(text.invalidDiscount);
    if (!direct && !reason.trim()) return setNotice(text.invalidReason);
    const terms: Record<string, number | null> = {};
    for (const provider of DASHBOARD_PROVIDERS) {
      const raw = providers[provider.id]?.trim() ?? "";
      if (!raw) continue;
      const value = parsed(raw);
      if (value === null) return setNotice(`${provider.name}: ${text.invalidDiscount}`);
      terms[provider.id] = value;
    }
    setBusy(true);
    try {
      if (direct) await api.referralSetBusinessPricing({ customerEmail: row.email!, discountPercent: base, providers: terms }, mutationKey());
      else await api.referralRequestB2B({ customerEmail: row.email!, requestType: row.customerType === "b2b" ? "b2b_pricing" : "b2b_conversion", requestedDiscountBps: base * 100, providers: Object.fromEntries(Object.entries(terms).map(([provider, value]) => [provider, value === null ? null : value * 100])), reason: reason.trim() }, mutationKey());
      onClose();
    } catch (cause) { setNotice(errorMessage(cause, text.mutationError)); }
    finally { setBusy(false); }
  }

  return <div className="modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget && !busy) onClose(); }}><section className="key-modal referral-modal referral-pricing-modal" role="dialog" aria-modal="true" aria-labelledby="business-pricing-title"><div className="key-modal-head"><div><span className="eyebrow">{text.businessTerms}</span><h2 id="business-pricing-title">{row.customerType === "b2b" ? direct ? text.editRates : text.requestRates : direct ? text.makeB2b : text.requestB2b}</h2><p className="referral-modal-email" translate="no">{row.email}</p></div><button ref={closeRef} type="button" className="key-modal-close" aria-label={text.cancel} disabled={busy} onClick={onClose}>×</button></div>
    <div className="referral-ceiling-callout"><span>{direct ? text.ceiling : language === "ru" ? "Лимит заявки" : "Request limit"}</span><strong>{ceilingPercent.toLocaleString(locale)}%</strong><small>{direct ? language === "ru" ? "Вы не сможете сохранить скидку выше личного лимита." : "You cannot save a discount above your personal limit." : language === "ru" ? "Администратор может одобрить другие условия." : "An administrator may approve different terms."}</small></div>
    <Field label={language === "ru" ? "Базовая скидка" : "Base discount"}><div className="referral-percent-input"><input type="text" inputMode="numeric" autoComplete="off" value={discount} onChange={(event) => setDiscount(event.target.value.replace(/\D/g, "").slice(0, 2))} placeholder="15" /><i>%</i></div></Field>
    <fieldset className="referral-provider-terms"><legend>{language === "ru" ? "Скидки по провайдерам — необязательно" : "Provider discounts — optional"}</legend>{DASHBOARD_PROVIDERS.map((provider) => <label key={provider.id} style={{ "--provider-color": provider.color, ...(provider.logo ? { "--provider-logo": `url("${provider.logo}")` } : {}) } as CSSProperties}>{provider.logo ? <span className="referral-provider-icon" aria-hidden="true" /> : <i aria-hidden="true">{provider.name.slice(0, 1)}</i>}<b>{provider.name}</b><div className="referral-percent-input"><input aria-label={`${provider.name} ${text.discount}`} type="text" inputMode="numeric" autoComplete="off" value={providers[provider.id] ?? ""} onChange={(event) => setProviders({ ...providers, [provider.id]: event.target.value.replace(/\D/g, "").slice(0, 2) })} placeholder={language === "ru" ? "База" : "Base"} /><i>%</i></div></label>)}</fieldset>
    {!direct && <Field label={text.reason}><textarea rows={4} maxLength={4_000} autoComplete="off" value={reason} onChange={(event) => setReason(event.target.value)} placeholder={text.reasonPlaceholder} /></Field>}
    {notice && <div className="referral-live bad" role="alert">{notice}</div>}
    <div className="key-modal-actions"><button type="button" className="btn btn-ghost" disabled={busy} onClick={onClose}>{text.cancel}</button><button type="button" className="btn btn-primary" disabled={busy} onClick={() => void save()}>{busy ? text.saving : direct ? text.save : text.sendRequest}</button></div>
  </section></div>;
}

// ---------------------------------------------------------------------------
// Team
// ---------------------------------------------------------------------------

function Team({ snapshot, language, refresh }: { snapshot: ReferralActiveSnapshot; language: Language; refresh(): Promise<void> }) {
  const text = copy[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const maxShare = Math.min(2_000, snapshot.membership.teamOverrideMaxBps);
  const maxB2b = snapshot.membership.b2bMaxDiscountBps;
  const initialAuthority = useMemo<ReferralAuthorityInput>(() => ({ teamOverrideMaxBps: 0, teamInvitesEnabled: false, b2bEnabled: false, b2bMaxDiscountBps: 0, b2bCanDelegate: false }), []);
  const [email, setEmail] = useState("");
  const [share, setShare] = useState(0);
  const [authority, setAuthority] = useState(initialAuthority);
  const [editing, setEditing] = useState<ReferralTeamMember | null>(null);
  const [revoking, setRevoking] = useState<{ id: string; email: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<{ kind: "ok" | "bad"; message: string } | null>(null);

  async function invite(event: FormEvent) {
    event.preventDefault(); setNotice(null);
    if (!validEmail(email)) return setNotice({ kind: "bad", message: text.invalidEmail });
    if (share < 0 || share > maxShare || authority.teamOverrideMaxBps > maxShare) return setNotice({ kind: "bad", message: text.invalidShare });
    setBusy(true);
    try { await api.referralInviteTeam({ email: email.trim(), overrideBps: share, authority }); setEmail(""); setShare(0); setAuthority(initialAuthority); await refresh(); setNotice({ kind: "ok", message: text.inviteSent }); }
    catch (cause) { setNotice({ kind: "bad", message: errorMessage(cause, text.mutationError) }); }
    finally { setBusy(false); }
  }

  async function revoke(id: string): Promise<boolean> {
    setBusy(true); setNotice(null);
    try { await api.referralRevokeInvitation(id); await refresh(); return true; }
    catch (cause) { setNotice({ kind: "bad", message: errorMessage(cause, text.mutationError) }); return false; }
    finally { setBusy(false); }
  }

  const pending = snapshot.invitations.filter((item) => !item.consumedAt && !item.revokedAt);
  return <div className="referral-tab-panel">
    <PageTitle title={text.teamTitle} sub={text.teamSub} />
    <StatStrip items={[
      { label: text.teamLimit, value: pct(maxShare, locale), foot: text.hardLimit, accent: true },
      { label: text.memberRate, value: pct(1_000, locale), foot: text.fixedRateHint },
      { label: text.activeMembers, value: snapshot.team.length.toLocaleString(locale), foot: text.directMembers },
      { label: text.pendingInvites, value: pending.length.toLocaleString(locale), foot: text.valid30 },
    ]} />
    <div className="rp-stack" style={{ marginTop: 24 }}>
      <Card title={text.programTerms}><p className="rp-card-sub">{text.termsBody}</p></Card>

      {snapshot.membership.teamInvitesEnabled ? <form className="rp-card referral-team-form" onSubmit={invite}>
        <h3 className="rp-card-title">{text.invite}</h3>
        <p className="rp-card-sub">{text.existingOnly}</p>
        <div className="referral-team-core"><Field label={text.email}><input name="teamEmail" type="email" autoComplete="email" spellCheck={false} maxLength={320} value={email} onChange={(event) => setEmail(event.target.value)} placeholder="name@company.com" translate="no" /></Field><PercentField label={text.retainedShare} value={share} max={maxShare} onChange={setShare} help={text.retainedHelp.replace("{max}", String(maxShare / 100))} /><PercentField label={text.delegatedTeamLimit} value={authority.teamOverrideMaxBps} max={maxShare} onChange={(teamOverrideMaxBps) => setAuthority({ ...authority, teamOverrideMaxBps })} help={text.delegatedTeamHelp} /></div>
        <div className="referral-member-rate"><span>{text.memberRate}</span><strong>{pct(1_000, locale)}</strong><small>{text.platformControlled}</small></div>
        <AuthorityFields value={authority} maxB2b={maxB2b} canDelegateB2b={snapshot.membership.b2bCanDelegate} onChange={setAuthority} language={language} />
        <div className="rp-actions"><button className="btn btn-primary" disabled={busy}>{busy ? text.inviting : text.sendInvitation}</button></div>
      </form> : <div className="rp-note">{language === "ru" ? "Приглашения в команду отключены администратором для вашего аккаунта." : "Team invitations are disabled for your account by an administrator."}</div>}
      <LiveNotice notice={notice} />

      <Card title={text.activeMembers} sub={`${snapshot.team.length.toLocaleString(locale)} · ${text.email}`}>
        <Table label={text.activeMembers} className="team-table" head={<><th>{text.email}</th><th className="rp-num">{text.retainedShare}</th><th className="rp-num">{text.referralsCount}</th><th className="rp-num">{text.memberNet}</th><th className="rp-num">{text.myShare}</th><th>{text.authority}</th></>}>
          {snapshot.team.length === 0 ? <EmptyRow span={6} title={text.noTeam} /> : snapshot.team.map((member, index) => <tr key={`${member.email ?? "unknown"}-${index}`}>
            <td><span className="referral-email" translate="no">{member.email ?? text.unknownEmail}</span><small>{pct(member.commissionBps, locale)} {text.fixedRate.toLocaleLowerCase()}</small></td>
            <td className="rp-num">{pct(member.overrideBps, locale)}</td>
            <td className="rp-num">{member.referredUsers.toLocaleString(locale)}</td>
            <td className="rp-num">{formatNanoUsd(member.theirNetNano, locale)}</td>
            <td className="rp-num rp-earned">{formatNanoUsd(member.myOverrideNetNano, locale)}</td>
            <td><button type="button" className="btn btn-ghost btn-sm" disabled={!member.email || busy} onClick={() => setEditing(member)}>{text.edit}</button></td>
          </tr>)}
        </Table>
      </Card>

      <Card title={text.pendingInvites} sub={text.existingOnly}>
        {pending.length === 0 ? <div className="rp-empty"><b>{text.noInvites}</b></div> : <div className="referral-invites">{pending.map((item) => <article className="referral-invite" key={item.id}><div><strong translate="no">{item.email ?? text.unknownEmail}</strong><span>{text.retainedShare}: {pct(item.overrideBps, locale)} · {text.inviteExpires}: {date(item.expiresAt, locale)}</span></div><button type="button" className="btn btn-ghost btn-sm" disabled={busy} onClick={() => setRevoking({ id: item.id, email: item.email ?? text.unknownEmail })}>{text.revoke}</button></article>)}</div>}
      </Card>
    </div>
    {editing && <TeamEditor member={editing} parent={snapshot} language={language} busy={busy} onClose={() => setEditing(null)} onSave={async (patch) => { if (!editing.email) return; setBusy(true); setNotice(null); try { await api.referralUpdateTeam({ email: editing.email, ...patch }); await refresh(); setEditing(null); setNotice({ kind: "ok", message: text.updateSaved }); } catch (cause) { setNotice({ kind: "bad", message: errorMessage(cause, text.mutationError) }); } finally { setBusy(false); } }} />}
    {revoking && <ConfirmDialog title={text.revokeInviteTitle} body={text.revokeInviteBody.replace("{email}", revoking.email)} confirm={text.confirmRevoke} cancel={text.cancel} busyLabel={text.saving} busy={busy} onClose={() => setRevoking(null)} onConfirm={async () => { if (await revoke(revoking.id)) setRevoking(null); }} />}
  </div>;
}

function AuthorityFields({ value, maxB2b, canDelegateB2b, onChange, language }: { value: ReferralAuthorityInput; maxB2b: number; canDelegateB2b: boolean; onChange(value: ReferralAuthorityInput): void; language: Language }) {
  const text = copy[language];
  return <fieldset className="referral-authority-grid"><legend>{language === "ru" ? "Права участника" : "Member permissions"}</legend><CheckboxCard name="teamInvitesEnabled" label={text.allowInvites} help={text.allowInvitesHelp} checked={value.teamInvitesEnabled} onChange={(teamInvitesEnabled) => onChange({ ...value, teamInvitesEnabled })} />{canDelegateB2b && <CheckboxCard name="b2bEnabled" label={text.allowB2b} help={text.allowB2bHelp} checked={value.b2bEnabled} onChange={(b2bEnabled) => onChange({ ...value, b2bEnabled, b2bMaxDiscountBps: b2bEnabled ? value.b2bMaxDiscountBps : 0, b2bCanDelegate: b2bEnabled ? value.b2bCanDelegate : false })} />}{canDelegateB2b && value.b2bEnabled && <div className="referral-authority-nested"><PercentField label={text.maxB2b} value={value.b2bMaxDiscountBps} max={maxB2b} onChange={(b2bMaxDiscountBps) => onChange({ ...value, b2bMaxDiscountBps })} help={`${text.ceiling}: ${pct(maxB2b, language === "ru" ? "ru-RU" : "en-US")}`} /><CheckboxCard name="b2bCanDelegate" label={text.allowB2bDelegate} help={text.allowB2bDelegateHelp} checked={value.b2bCanDelegate} onChange={(b2bCanDelegate) => onChange({ ...value, b2bCanDelegate })} /></div>}</fieldset>;
}

function TeamEditor({ member, parent, language, busy, onClose, onSave }: { member: ReferralTeamMember; parent: ReferralActiveSnapshot; language: Language; busy: boolean; onClose(): void; onSave(patch: { overrideBps: number } & ReferralAuthorityInput): Promise<void> }) {
  const text = copy[language];
  const maxShare = Math.min(2_000, parent.membership.teamOverrideMaxBps);
  const [share, setShare] = useState(member.overrideBps);
  const [authority, setAuthority] = useState<ReferralAuthorityInput>({ teamOverrideMaxBps: member.teamOverrideMaxBps, teamInvitesEnabled: member.teamInvitesEnabled, b2bEnabled: member.b2bEnabled, b2bMaxDiscountBps: member.b2bMaxDiscountBps, b2bCanDelegate: member.b2bCanDelegate });
  const closeRef = useModalFocus(onClose);
  return <div className="modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}><section className="key-modal referral-modal" role="dialog" aria-modal="true" aria-labelledby="team-editor-title"><div className="key-modal-head"><div><span className="eyebrow">{text.team}</span><h2 id="team-editor-title" translate="no">{member.email}</h2></div><button ref={closeRef} type="button" className="key-modal-close" aria-label={text.cancel} onClick={onClose}>×</button></div><div className="referral-team-core"><PercentField label={text.retainedShare} value={share} max={maxShare} onChange={setShare} help={text.retainedHelp.replace("{max}", String(maxShare / 100))} /><ReadOnly label={text.memberRate} value={`${pct(member.commissionBps, language === "ru" ? "ru-RU" : "en-US")} · ${text.fixedRateHint}`} /><PercentField label={text.delegatedTeamLimit} value={authority.teamOverrideMaxBps} max={maxShare} onChange={(teamOverrideMaxBps) => setAuthority({ ...authority, teamOverrideMaxBps })} help={text.delegatedTeamHelp} /></div><AuthorityFields value={authority} maxB2b={parent.membership.b2bMaxDiscountBps} canDelegateB2b={parent.membership.b2bCanDelegate} onChange={setAuthority} language={language} /><div className="key-modal-actions"><button type="button" className="btn btn-ghost" onClick={onClose}>{text.cancel}</button><button type="button" className="btn btn-primary" disabled={busy || share > maxShare || authority.teamOverrideMaxBps > maxShare} onClick={() => void onSave({ overrideBps: share, ...authority })}>{busy ? text.saving : text.save}</button></div></section></div>;
}

function useModalFocus(onClose: () => void) {
  const focusRef = useRef<HTMLButtonElement>(null);
  const closeRef = useRef(onClose);
  useEffect(() => { closeRef.current = onClose; }, [onClose]);
  useEffect(() => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    focusRef.current?.focus();
    const handleKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") { closeRef.current(); return; }
      if (event.key !== "Tab") return;
      const modal = focusRef.current?.closest<HTMLElement>("[role='dialog'],[role='alertdialog']");
      const controls = modal ? [...modal.querySelectorAll<HTMLElement>("button:not(:disabled),a[href],input:not(:disabled),select:not(:disabled),textarea:not(:disabled),[tabindex]:not([tabindex='-1'])")] : [];
      if (controls.length === 0) return;
      const first = controls[0]!;
      const last = controls[controls.length - 1]!;
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
    };
    document.addEventListener("keydown", handleKey);
    return () => { document.removeEventListener("keydown", handleKey); document.body.style.overflow = previousOverflow; previous?.focus(); };
  }, []);
  return focusRef;
}

function ConfirmDialog({ title, body, confirm, cancel, busyLabel, busy, onClose, onConfirm }: { title: string; body: string; confirm: string; cancel: string; busyLabel: string; busy: boolean; onClose(): void; onConfirm(): Promise<void> }) {
  const cancelRef = useModalFocus(() => { if (!busy) onClose(); });
  return <div className="modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget && !busy) onClose(); }}><section className="key-modal key-revoke-modal" role="alertdialog" aria-modal="true" aria-labelledby="referral-confirm-title" aria-describedby="referral-confirm-body"><div className="key-modal-head"><div><span className="eyebrow">{confirm}</span><h2 id="referral-confirm-title">{title}</h2></div><button ref={cancelRef} type="button" className="key-modal-close" aria-label={cancel} disabled={busy} onClick={onClose}>×</button></div><p id="referral-confirm-body">{body}</p><div className="key-modal-actions"><button type="button" className="btn btn-ghost" disabled={busy} onClick={onClose}>{cancel}</button><button type="button" className="btn btn-danger" disabled={busy} onClick={() => void onConfirm()}>{busy ? busyLabel : confirm}</button></div></section></div>;
}

// ---------------------------------------------------------------------------
// Payouts
// ---------------------------------------------------------------------------

function Payouts({ snapshot, language, refresh }: { snapshot: ReferralActiveSnapshot; language: Language; refresh(): Promise<void> }) {
  const text = copy[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const bound = snapshot.membership.payoutDetails?.address ?? null;
  const [wallet, setWallet] = useState(bound ?? "");
  const [editingWallet, setEditingWallet] = useState(!bound);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<{ kind: "ok" | "bad"; message: string } | null>(null);

  async function save(event: FormEvent) {
    event.preventDefault(); setNotice(null);
    if (!/^0x[a-fA-F0-9]{40}$/.test(wallet)) return setNotice({ kind: "bad", message: text.invalidWallet });
    setBusy(true);
    try { await api.referralUpdateWallet(wallet); await refresh(); setEditingWallet(false); setNotice({ kind: "ok", message: text.walletSaved }); }
    catch (cause) { setNotice({ kind: "bad", message: errorMessage(cause, text.mutationError) }); }
    finally { setBusy(false); }
  }

  const lockedTotal = snapshot.period.locked.reduce((sum, period) => sum + BigInt(period.netNano), 0n);
  return <div className="referral-tab-panel">
    <PageTitle title={text.payoutTitle} sub={text.payoutSub} />
    <StatStrip items={[
      { label: text.currentPeriod, value: formatNanoUsd(snapshot.period.current.netNano, locale), foot: `${date(snapshot.period.current.start, locale)} — ${date(snapshot.period.current.end, locale)}`, accent: true },
      { label: text.locked, value: formatNanoUsd(lockedTotal, locale), foot: snapshot.period.locked[0] ? `${text.unlocks} ${date(snapshot.period.locked[0].unlocksAt, locale)}` : `${snapshot.payoutPolicy.lockDays} ${language === "ru" ? "дней" : "days"}` },
      { label: text.nextPayout, value: formatNanoUsd(snapshot.period.nextPayout.estimatedNano, locale), foot: date(snapshot.period.nextPayout.date, locale) },
      { label: text.availableToPay, value: formatNanoUsd(snapshot.period.payableNano, locale), foot: `${formatNanoUsd(snapshot.period.lifetimePaidNano, locale)} ${text.lifetimePaid.toLocaleLowerCase()}` },
    ]} />

    <div className="rp-stack" style={{ marginTop: 24 }}>
      <PayoutTimeline snapshot={snapshot} language={language} />

      {BigInt(snapshot.period.debtNano) > 0n && <div className="rp-note bad" role="alert"><strong>{text.debt}: {formatNanoUsd(snapshot.period.debtNano, locale)}.</strong>{" "}{language === "ru" ? "Будущие начисления сначала погасят долг; автоматического списания с кошелька нет." : "Future earnings repay it first; the external wallet is never debited automatically."}</div>}

      <form className="rp-card" onSubmit={save}>
        <h3 className="rp-card-title">{text.wallet}</h3>
        <p className="rp-card-sub">{text.walletHelp}</p>
        {!bound && <div className="rp-note" style={{ marginBottom: 16 }}>{text.walletUnbound}</div>}
        {bound && !editingWallet ? <div className="rp-wallet-row">
          <span className="rp-wallet-chip"><span className="rp-wallet-net">BSC · USDT BEP-20</span><span className="rp-wallet-addr" translate="no" title={bound}>{bound}</span></span>
          <button type="button" className="btn btn-ghost btn-sm" onClick={() => setEditingWallet(true)}>{text.changeWallet}</button>
        </div> : <>
          <Field label={text.wallet}><input name="payoutWallet" type="text" autoComplete="off" spellCheck={false} inputMode="text" value={wallet} onChange={(event) => setWallet(event.target.value.trim())} placeholder="0x0000000000000000000000000000000000000000" translate="no" /></Field>
          <div className="rp-actions">{bound && <button type="button" className="btn btn-ghost" disabled={busy} onClick={() => { setWallet(bound); setEditingWallet(false); }}>{text.cancel}</button>}<button className="btn btn-primary" disabled={busy}>{busy ? text.saving : text.saveWallet}</button></div>
        </>}
        <LiveNotice notice={notice} />
      </form>

      <Card title={text.payments} sub={text.paymentsSub}>
        <Table label={text.payments} head={<><th>{text.created}</th><th className="rp-num">{text.amount}</th><th>{text.status}</th><th>{text.tx}</th></>}>
          {snapshot.payouts.length === 0 ? <EmptyRow span={4} title={text.noPayouts} /> : snapshot.payouts.map((payout) => <tr key={payout.id}>
            <td>{date(payout.paidAt ?? payout.requestedAt, locale)}</td>
            <td className="rp-num rp-earned">{formatNanoUsd(payout.amountNano, locale)}</td>
            <td><Status value={payoutStatusLabel(payout.status, language)} kind={payout.status === "paid" ? "ok" : payout.status === "rejected" ? "bad" : "warn"} /></td>
            <td><span className="referral-email referral-tx" title={payout.txHash ?? undefined} translate="no">{payout.txHash ? `${payout.txHash.slice(0, 12)}…` : "—"}</span></td>
          </tr>)}
        </Table>
      </Card>

      <Card title={text.payoutHowTitle}>
        <ul className="rp-how">
          <li>{text.payoutHowList}</li>
          <li>{text.payoutHowWallet}</li>
          <li>{text.minimum}: <strong>{formatNanoUsd(snapshot.payoutPolicy.minPayoutNano, locale)}</strong>.</li>
        </ul>
      </Card>

      <Card title={text.periodHistory} sub={text.periodHistorySub}>
        <Table label={text.periodHistory} head={<><th>{text.currentPeriod}</th><th>{text.phase}</th><th>{text.payoutDate}</th><th className="rp-num">{text.earned}</th><th className="rp-num">{text.adjustments}</th><th className="rp-num">{text.net}</th></>}>
          {snapshot.periodHistory.length === 0 ? <EmptyRow span={6} title={text.noEarnings} /> : snapshot.periodHistory.map((period) => <tr key={`${period.key}-${period.index}`}>
            <td>{period.key} · {period.index}/2</td>
            <td><Status value={periodPhaseLabel(period.phase, language)} /></td>
            <td>{date(period.payoutDate, locale)}</td>
            <td className="rp-num">{formatNanoUsd(period.earnedNano, locale)}</td>
            <td className="rp-num">{formatNanoUsd(period.adjustmentNano, locale)}</td>
            <td className="rp-num rp-earned">{formatNanoUsd(period.netNano, locale)}</td>
          </tr>)}
        </Table>
      </Card>
    </div>
  </div>;
}

function PayoutTimeline({ snapshot, language }: { snapshot: ReferralActiveSnapshot; language: Language }) {
  const text = copy[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const day = 86_400_000;
  const now = new Date(snapshot.period.now).getTime();
  const current = { key: snapshot.period.current.key, start: new Date(snapshot.period.current.start).getTime(), end: new Date(snapshot.period.current.end).getTime(), amount: snapshot.period.current.netNano, current: true };
  const history = snapshot.periodHistory.filter((item) => item.phase === "locked" || item.phase === "payable").map((item) => ({ key: `${item.key}-${item.index}`, start: new Date(item.start).getTime(), end: new Date(item.end).getTime(), amount: item.netNano, current: false }));
  const lanes = [...history, current].sort((left, right) => left.start - right.start);
  const windowEnd = (lane: typeof current) => lane.end + (snapshot.payoutPolicy.lockDays + snapshot.payoutPolicy.windowDays) * day;
  const axisStart = Math.min(...lanes.map((lane) => lane.start));
  const axisEnd = Math.max(now, ...lanes.map(windowEnd));
  const span = Math.max(1, axisEnd - axisStart);
  const left = (value: number) => `${Math.max(0, Math.min(100, (value - axisStart) / span * 100))}%`;
  const width = (start: number, end: number) => `${Math.max(0, (end - start) / span * 100)}%`;
  const short = (value: number) => new Date(value).toLocaleDateString(locale, { day: "numeric", month: "short", timeZone: "UTC" });
  return <section className="referral-payout-roadmap" aria-labelledby="payout-roadmap-title"><div className="referral-roadmap-head"><h3 id="payout-roadmap-title">{text.payoutRoadmap}</h3><div><span><i className="accrue" />{text.accruing}</span><span><i className="lock" />{text.lock7}</span><span><i className="pay" />{text.pay3}</span></div></div><div className="referral-roadmap-chart"><div className="referral-now-line" style={{ left: left(now) }}><span>{text.now}</span></div>{lanes.map((lane) => <div className="referral-roadmap-lane" key={lane.key}><div className="referral-roadmap-label"><strong>{short(lane.start)} — {short(lane.end - day)}</strong><span>{formatNanoUsd(lane.amount, locale)}{lane.current ? ` · ${text.currentPeriod.toLocaleLowerCase()}` : ""}</span></div><div className="referral-roadmap-track"><i className="accrue" style={{ left: left(lane.start), width: width(lane.start, lane.end) }}><span>{text.accruing}</span></i><i className="lock" style={{ left: left(lane.end), width: width(lane.end, lane.end + snapshot.payoutPolicy.lockDays * day) }}><span>{text.lock7}</span></i><i className="pay" style={{ left: left(lane.end + snapshot.payoutPolicy.lockDays * day), width: width(lane.end + snapshot.payoutPolicy.lockDays * day, windowEnd(lane)) }}><span>{text.pay3}</span></i></div></div>)}<div className="referral-roadmap-axis"><span>{short(axisStart)}</span><span>{short(axisStart + span / 2)}</span><span>{short(axisEnd)}</span></div></div></section>;
}

// ---------------------------------------------------------------------------
// Docs — the partner cabinet documentation, numbered and personalised
// ---------------------------------------------------------------------------

function PartnerDocs({ snapshot, language }: { snapshot: ReferralActiveSnapshot; language: Language }) {
  const text = copy[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const rate = pct(snapshot.membership.commissionBps, locale);
  const example = formatNanoUsd(commissionOnHundred(snapshot.membership.commissionBps), locale);
  const referralUrl = `${PARTNER_SITE_ORIGIN}/?ref=${snapshot.membership.referralCode}`;
  const ru = language === "ru";

  const sections: Array<{ title: string; intro?: ReactNode; body: ReactNode[] }> = [
    { title: text.docsEarn, body: [
      text.docsEarnBody,
      <>{ru ? "Ваша текущая ставка: " : "Your current rate: "}<strong>{rate}</strong>{ru ? " от оплаченного использования ваших рефералов." : " of your referrals' paid API usage."}</>,
      <>{ru ? "Бесплатные средства платформы не считаются: они тратятся первыми, и комиссию приносит только использование, оплаченное деньгами клиента." : "Free platform credit never counts: it is spent first, and only usage covered by the referral's own money earns commission."}</>,
    ] },
    { title: text.docsIdentity, body: [
      text.docsIdentityBody,
      <>{ru ? "Ваш код — " : "Your code is "}<b className="mono" translate="no">{snapshot.membership.referralCode}</b>{ru ? "; ссылку " : "; the link "}<b className="mono" translate="no">{referralUrl}</b>{ru ? " можно скопировать на вкладке «Обзор»." : " can be copied on the Overview tab."}</>,
      <>{ru ? "Привязка влияет только на то, кому идёт комиссия — цену и скидку клиента она не меняет." : "Attribution only affects who gets the commission — it never changes the client's price or discount."}</>,
    ] },
    { title: text.docsFormula, intro: <CommissionFormula snapshot={snapshot} language={language} />, body: [
      text.docsFormulaBody,
      <>{ru ? "Комиссия = " : "Commission = "}<strong>{rate}</strong>{ru ? " × оплаченное реальными деньгами использование." : " × their real-money paid usage."}</>,
      <>{ru ? "Пример: реферал тратит $100 реальных денег на API → вы получаете " : "Example: a referral spends $100 of real money on the API → you earn "}<strong>{example}</strong>.</>,
      <>{ru ? "Если платёж клиента возвращён, профинансированная им комиссия отменяется. Уже выплаченная сумма становится долгом, который погашают будущие начисления; внешний кошелёк не списывается." : "If a client payment is reversed, the commission it funded is reversed too. An already-paid amount becomes a debt that future earnings repay; the external wallet is never debited."}</>,
    ] },
    { title: text.docsTeam, body: [
      text.docsTeamBody,
      <>{ru ? "Ваш лимит удержания: " : "Your retained-share limit: "}<strong>{pct(Math.min(2_000, snapshot.membership.teamOverrideMaxBps), locale)}</strong>.</>,
    ] },
    { title: text.docsB2b, body: [
      text.docsB2bBody,
      <>{ru ? "Ваш B2B-лимит: " : "Your B2B ceiling: "}<strong>{pct(snapshot.membership.b2bMaxDiscountBps, locale)}</strong>.</>,
    ] },
    { title: text.docsWallet, body: [text.docsWalletBody, text.payoutHowWallet] },
    { title: text.docsSchedule, body: [
      text.docsScheduleBody,
      text.payoutHowList,
      <>{text.minimum}: <strong>{formatNanoUsd(snapshot.payoutPolicy.minPayoutNano, locale)}</strong>.</>,
    ] },
    { title: text.docsPrivacy, body: [text.docsPrivacyBody] },
  ];

  return <div className="referral-tab-panel">
    <PageTitle title={text.docsTitle} sub={text.docsSub} />
    <div className="rp-stack">
      {sections.map((section) => <Card key={section.title} title={section.title}>
        {section.intro && <div style={{ marginBottom: 16 }}>{section.intro}</div>}
        <ul className="rp-how">{section.body.map((item, position) => <li key={position}>{item}</li>)}</ul>
      </Card>)}
    </div>
  </div>;
}

// ---------------------------------------------------------------------------
// Small shared controls
// ---------------------------------------------------------------------------

function Field({ label, children }: { label: string; children: ReactNode }) { return <label className="referral-field"><span>{label}</span>{children}</label>; }
function ReadOnly({ label, value }: { label: string; value: string }) { return <div className="referral-field referral-readonly"><span>{label}</span><strong>{value}</strong></div>; }
function PercentField({ label, value, max, help, onChange }: { label: string; value: number; max: number; help?: string; onChange(value: number): void }) { return <label className="referral-field"><span>{label}</span><div className="referral-percent-input"><input name={label.replaceAll(" ", "-")} type="number" min={0} max={max / 100} step={0.01} inputMode="decimal" autoComplete="off" value={value / 100} onChange={(event) => onChange(Math.round(Number(event.target.value || 0) * 100))} /><i>%</i></div>{help && <small>{help}</small>}</label>; }
function CheckboxCard({ name, label, help, checked, onChange }: { name: string; label: string; help: string; checked: boolean; onChange(value: boolean): void }) { return <label className={`referral-checkbox-card${checked ? " checked" : ""}`}><input name={name} type="checkbox" checked={checked} onChange={(event) => onChange(event.target.checked)} /><span><b>{label}</b><small>{help}</small></span></label>; }
function Status({ value, kind }: { value: string; kind?: "ok" | "bad" | "warn" }) { return <span className={`referral-status${kind ? ` ${kind}` : ""}`}>{value}</span>; }
function LiveNotice({ notice }: { notice: { kind: "ok" | "bad"; message: string } | null }) { return <div className={`rp-live${notice ? ` ${notice.kind}` : ""}`} aria-live="polite">{notice?.message ?? ""}</div>; }
function periodPhaseLabel(value: ReferralActiveSnapshot["periodHistory"][number]["phase"], language: Language): string { const labels = language === "ru" ? { accruing: "Начисляется", locked: "Заблокирован", payable: "К выплате", closed: "Закрыт" } : { accruing: "Accruing", locked: "Locked", payable: "Payable", closed: "Closed" }; return labels[value]; }
function payoutStatusLabel(value: ReferralActiveSnapshot["payouts"][number]["status"], language: Language): string { const labels = language === "ru" ? { requested: "Запрошена", approved: "Одобрена", paid: "Выплачена", rejected: "Отклонена" } : { requested: "Requested", approved: "Approved", paid: "Paid", rejected: "Rejected" }; return labels[value]; }
