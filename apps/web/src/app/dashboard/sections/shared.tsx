"use client";

import { useState } from "react";
import { useI18n } from "@/components/i18n-provider";
import { dashboardCopy, type DashboardCopy } from "@/lib/dashboard-copy";

export const NANO_PER_USD = 1_000_000_000n;
export const BASIS_POINTS = 10_000n;

export const localDashboardCopy = {
  en: {
    logoutError: "Logout failed. Your server session is still active; please try again.", loggingOut: "Logging out…",
    invalidCheckoutUrl: "The payment provider returned an unsafe checkout address. Payment was not opened.",
    invalidWholeUsd: "Enter a positive whole USD amount using digits only, without decimals, signs, separators, or leading zeros.",
    editKey: "Edit", editKeyTitle: "Edit API key", editKeyHelp: "Update the name, spending limit, or expiration in one place. Limit changes apply to new requests immediately.", labelRequired: "Enter a label before saving.", updateKeyError: "Unable to update API key",
    filterLabel: "Filter API keys", currentFilter: "Current", workingFilter: "Working", attentionFilter: "Needs attention", disabledFilter: "Revoked", allFilter: "All",
    noActiveKeys: "No current API keys.", noWorkingKeys: "No working API keys.", noAttentionKeys: "No keys need attention.", noDisabledKeys: "No revoked API keys.", activeStatus: "Working", disabledStatus: "Revoked",
    createKey: "Create key", createKeyTitle: "Create an API key", createKeyHelp: "Add optional guardrails now. The secret is shown only once.",
    keyName: "Key name", keyNameHint: "For example, Production or CI", keyNameHelp: "Use the environment or tool name so this credential stays recognizable.", guardrailsTitle: "Usage guardrails", guardrailsHelp: "Optional limits protect a leaked or forgotten credential without affecting your other keys.", spendLimit: "Spending limit", spendLimitHint: "Lifetime platform spend cap in USD", optional: "Optional", expiration: "Expiration date", noExpiration: "Never expires", expirationHint: "Expires at the end of this day in your local time.",
    cancel: "Cancel", creating: "Creating…", invalidSpendLimit: "Enter a positive USD amount with up to 2 decimals.", invalidExpiration: "Choose a future expiration date.",
    committedSpend: "Billed and reserved", policyLimitHint: "Leave empty for unlimited. Up to 9 decimal places.", policyExpirationHint: "Leave empty to keep this key from expiring.",
    savePolicy: "Save changes", savingPolicy: "Saving…", invalidPolicySpendLimit: "Enter a positive USD amount with up to 9 decimals.",
    policyBelowCommitted: "The limit cannot be below billed and reserved usage ({amount}).", policyReactivates: "Increasing or removing this guardrail can make the key usable immediately.",
    searchKeys: "Search by name or key suffix", sortBy: "Sort by", sortNewest: "Newest", sortName: "Name", sortSpend: "Highest spend", sortLastUsed: "Recently billed",
    keysListTitle: "Your API keys", keysListSummary: "Showing {shown} of {total} keys",
    colName: "Integration", colKey: "Credential", colLastUsed: "Last billed", colSpend: "Usage", colLimit: "Limit", colExpires: "Expires", colStatus: "Status", colActions: "Actions",
    spentOfLimit: "{spent} of {limit}", spentWithoutLimit: "{spent} spent · no limit", createdOn: "Created {date}", createFirstKey: "Create your first key", clearSearch: "Clear search", viewCurrentKeys: "View current keys",
    never: "Never", neverUsed: "No billed usage", unlimited: "Unlimited", expiredStatus: "Expired", limitReachedStatus: "Limit reached", expiresSoonStatus: "Expires soon", nearLimitStatus: "Near limit", moreActions: "More actions", openDocs: "Integration guide", revokeKey: "Revoke key",
    revokeTitle: "Revoke this key?", revokeBody: "Requests using this key will stop immediately. This action cannot be undone.", confirmRevoke: "Revoke key", noSearchResults: "No API keys match your search.",
    partialLedger: "Showing only the latest 100 ledger entries. Earlier transaction and top-up details are not shown.",
    payWith: "Payment method",
    twoFactorQrAlt: "QR code for authenticator app setup",
  },
  ru: {
    logoutError: "Не удалось выйти. Серверная сессия всё ещё активна; повторите попытку.", loggingOut: "Выходим…",
    invalidCheckoutUrl: "Платёжный сервис вернул небезопасный адрес. Страница оплаты не была открыта.",
    invalidWholeUsd: "Введите целую положительную сумму в USD только цифрами: без дробей, знаков, разделителей и ведущих нулей.",
    editKey: "Изменить", editKeyTitle: "Изменить API-ключ", editKeyHelp: "Измените название, лимит расходов или срок действия. Ограничения сразу применяются к новым запросам.", labelRequired: "Введите название перед сохранением.", updateKeyError: "Не удалось изменить API-ключ",
    filterLabel: "Фильтр API-ключей", currentFilter: "Текущие", workingFilter: "Работают", attentionFilter: "Требуют внимания", disabledFilter: "Отозваны", allFilter: "Все",
    noActiveKeys: "Текущих API-ключей нет.", noWorkingKeys: "Работающих API-ключей нет.", noAttentionKeys: "Нет ключей, требующих внимания.", noDisabledKeys: "Отозванных API-ключей нет.", activeStatus: "Работает", disabledStatus: "Отозван",
    createKey: "Создать ключ", createKeyTitle: "Создать API-ключ", createKeyHelp: "При необходимости задайте ограничения. Секрет будет показан только один раз.",
    keyName: "Название ключа", keyNameHint: "Например, Production или CI", keyNameHelp: "Укажите среду или инструмент, чтобы потом легко узнать этот ключ.", guardrailsTitle: "Ограничения использования", guardrailsHelp: "Необязательные ограничения защищают забытый или утёкший ключ, не затрагивая остальные.", spendLimit: "Лимит расходов", spendLimitHint: "Общий лимит расходов платформы в USD", optional: "Необязательно", expiration: "Дата истечения", noExpiration: "Без срока", expirationHint: "Ключ истечёт в конце выбранного дня по вашему местному времени.",
    cancel: "Отмена", creating: "Создаём…", invalidSpendLimit: "Введите положительную сумму USD максимум с 2 знаками после запятой.", invalidExpiration: "Выберите будущую дату истечения.",
    committedSpend: "Списано и зарезервировано", policyLimitHint: "Оставьте пустым, чтобы убрать лимит. До 9 знаков после запятой.", policyExpirationHint: "Оставьте пустым, чтобы ключ не истекал.",
    savePolicy: "Сохранить изменения", savingPolicy: "Сохраняем…", invalidPolicySpendLimit: "Введите положительную сумму USD максимум с 9 знаками после запятой.",
    policyBelowCommitted: "Лимит не может быть меньше уже списанной и зарезервированной суммы ({amount}).", policyReactivates: "Повышение или снятие ограничения может сразу снова активировать ключ.",
    searchKeys: "Поиск по имени или концу ключа", sortBy: "Сортировка", sortNewest: "Сначала новые", sortName: "По названию", sortSpend: "По расходам", sortLastUsed: "Недавние списания",
    keysListTitle: "Ваши API-ключи", keysListSummary: "Показано {shown} из {total}",
    colName: "Интеграция", colKey: "Учётные данные", colLastUsed: "Последнее списание", colSpend: "Использование", colLimit: "Лимит", colExpires: "Истекает", colStatus: "Статус", colActions: "Действия",
    spentOfLimit: "{spent} из {limit}", spentWithoutLimit: "Потрачено {spent} · без лимита", createdOn: "Создан {date}", createFirstKey: "Создать первый ключ", clearSearch: "Очистить поиск", viewCurrentKeys: "Показать текущие ключи",
    never: "Никогда", neverUsed: "Списаний не было", unlimited: "Без лимита", expiredStatus: "Истёк", limitReachedStatus: "Лимит исчерпан", expiresSoonStatus: "Скоро истечёт", nearLimitStatus: "Лимит близко", moreActions: "Другие действия", openDocs: "Инструкция подключения", revokeKey: "Отозвать ключ",
    revokeTitle: "Отозвать этот ключ?", revokeBody: "Запросы с этим ключом сразу перестанут работать. Действие нельзя отменить.", confirmRevoke: "Отозвать ключ", noSearchResults: "По вашему запросу ключи не найдены.",
    partialLedger: "Показаны только последние 100 записей журнала. Более ранние операции и пополнения не показаны.",
    payWith: "Способ оплаты",
    twoFactorQrAlt: "QR-код для настройки приложения аутентификации",
  },
} as const;

export function useDashboardCopy(): DashboardCopy {
  const { language } = useI18n();
  return dashboardCopy[language];
}

export function PageHeading({ eyebrow, title, subtitle }: { eyebrow: string; title: string; subtitle: string }) {
  return <header className="page-heading"><span className="eyebrow">{eyebrow}</span><h1 className="p-h1">{title}</h1><p className="p-sub">{subtitle}</p></header>;
}

// Section-level loading placeholders: same silhouette as the final layout so the
// panel does not jump when data lands (replaces the previous blank panel + text banner).

export function Stat({ label, value, detail, onClick }: { label: string; value: string; detail: string; onClick?: () => void }) { return <div className="ovstat"><span className="dlabel">{label}</span><b className="num">{value}</b>{onClick ? <button className="dtrend link plain-button" onClick={onClick}>{detail}</button> : <span className="dtrend">{detail}</span>}</div>; }

export function CopyButton({ value, className, label, copiedLabel }: { value: string; className?: string; label?: string; copiedLabel?: string }) {
  const copyText = useDashboardCopy();
  const [copied, setCopied] = useState(false);
  async function copy() {
    let successful = false;
    try {
      await navigator.clipboard.writeText(value);
      successful = true;
    } catch {
      const fallback = document.createElement("textarea");
      fallback.value = value;
      fallback.setAttribute("readonly", "");
      fallback.style.position = "fixed";
      fallback.style.opacity = "0";
      document.body.appendChild(fallback);
      fallback.select();
      successful = document.execCommand("copy");
      fallback.remove();
    }
    if (!successful) return;
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1_200);
  }
  return <button type="button" aria-live="polite" className={`btn btn-ghost btn-sm${className ? ` ${className}` : ""}`} onClick={copy}>{copied ? (copiedLabel ?? copyText.copied) : (label ?? copyText.copy)}</button>;
}

export function formatLedgerTime(timestamp: string, language: "en" | "ru"): string {
  const numeric = Number(timestamp);
  const milliseconds = numeric < 10_000_000_000 ? numeric * 1_000 : numeric;
  return new Date(milliseconds).toLocaleString(language === "ru" ? "ru-RU" : "en-US");
}

export function interpolate(template: string, values: Record<string, string | number>): string {
  return Object.entries(values).reduce((value, [key, replacement]) => value.replaceAll(`{${key}}`, String(replacement)), template);
}

export function roundDivide(numerator: bigint, denominator: bigint): bigint {
  if (denominator <= 0n) throw new Error("denominator must be positive");
  const negative = numerator < 0n;
  const absolute = negative ? -numerator : numerator;
  const rounded = (absolute + denominator / 2n) / denominator;
  return negative ? -rounded : rounded;
}
export function formatNanoUsd(value: string | bigint, locale: string, minimumFractionDigits = 0, maximumFractionDigits = 2): string {
  const nano = typeof value === "bigint" ? value : BigInt(value);
  const negative = nano < 0n;
  const absolute = negative ? -nano : nano;
  const digits = Math.max(0, Math.min(9, maximumFractionDigits));
  const minimum = Math.max(0, Math.min(digits, minimumFractionDigits));
  const quantum = 10n ** BigInt(9 - digits);
  const scaled = (absolute + quantum / 2n) / quantum;
  const units = 10n ** BigInt(digits);
  const whole = scaled / units;
  let fraction = digits > 0 ? (scaled % units).toString().padStart(digits, "0") : "";
  while (fraction.length > minimum && fraction.endsWith("0")) fraction = fraction.slice(0, -1);
  return `${negative ? "-" : ""}$${whole.toLocaleString(locale)}${fraction ? `.${fraction}` : ""}`;
}
export function compareBigInt(left: bigint, right: bigint): number { return left < right ? -1 : left > right ? 1 : 0; }
