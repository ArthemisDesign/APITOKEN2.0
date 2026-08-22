"use client";

import { useState, type ReactNode, type ButtonHTMLAttributes, type InputHTMLAttributes, type SelectHTMLAttributes, type TextareaHTMLAttributes } from "react";
import { useI18n } from "@/components/i18n";

// ---------------------------------------------------------------------------
// Button
// ---------------------------------------------------------------------------

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "ghost" | "danger";
  size?: "sm" | "md" | "lg";
  loading?: boolean;
};

export function Button({
  variant = "primary",
  size = "md",
  loading = false,
  className = "",
  children,
  disabled,
  ...rest
}: ButtonProps) {
  const cls = [
    "btn",
    `btn-${variant}`,
    size === "sm" ? "btn-sm" : size === "lg" ? "btn-lg" : "",
    className,
  ]
    .filter(Boolean)
    .join(" ");
  return (
    <button className={cls} disabled={disabled || loading} {...rest}>
      {loading ? <span className="spinner" aria-hidden /> : null}
      {children}
    </button>
  );
}

// ---------------------------------------------------------------------------
// Card
// ---------------------------------------------------------------------------

export function Card({
  title,
  sub,
  children,
  className = "",
}: {
  title?: ReactNode;
  sub?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section className={`card ${className}`.trim()}>
      {title ? <h2 className="card-title">{title}</h2> : null}
      {sub ? <p className="card-sub">{sub}</p> : null}
      {children}
    </section>
  );
}

// ---------------------------------------------------------------------------
// Form fields
// ---------------------------------------------------------------------------

export function Field({
  label,
  htmlFor,
  hint,
  children,
}: {
  label: string;
  htmlFor: string;
  hint?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="field">
      <label htmlFor={htmlFor}>{label}</label>
      {children}
      {hint ? <span className="field-hint">{hint}</span> : null}
    </div>
  );
}

export function Input(props: InputHTMLAttributes<HTMLInputElement>) {
  const { className = "", ...rest } = props;
  return <input {...rest} name={rest.name ?? rest.id} className={`input ${className}`.trim()} />;
}

export function Select(props: SelectHTMLAttributes<HTMLSelectElement>) {
  const { className = "", children, ...rest } = props;
  return (
    <select {...rest} name={rest.name ?? rest.id} className={`select ${className}`.trim()}>
      {children}
    </select>
  );
}

export function Textarea(props: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  const { className = "", ...rest } = props;
  return <textarea {...rest} name={rest.name ?? rest.id} className={`textarea ${className}`.trim()} />;
}

// ---------------------------------------------------------------------------
// Notice (inline toast/alert)
// ---------------------------------------------------------------------------

export function Notice({
  kind = "info",
  children,
}: {
  kind?: "error" | "success" | "info";
  children: ReactNode;
}) {
  return (
    <div className={`notice notice-${kind}`} role={kind === "error" ? "alert" : "status"}>
      {children}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Badge
// ---------------------------------------------------------------------------

export function Badge({
  tone = "neutral",
  children,
}: {
  tone?: "neutral" | "green" | "red" | "yellow";
  children: ReactNode;
}) {
  const cls = tone === "neutral" ? "badge" : `badge badge-${tone}`;
  return <span className={cls}>{children}</span>;
}

export function StatusBadge({ status }: { status: string }) {
  const { t } = useI18n();
  const s = status.toLowerCase();
  const tone =
    s === "active" || s === "approved" || s === "paid" || s === "completed"
      ? "green"
      : s === "pending" || s === "requested" || s === "processing"
        ? "yellow"
        : s === "rejected" || s === "suspended" || s === "blocked" || s === "disabled"
          ? "red"
          : "neutral";
  const label: Record<string, string> = {
    active: t("Active", "Активен"),
    approved: t("Approved", "Одобрено"),
    paid: t("Paid", "Выплачено"),
    completed: t("Completed", "Завершено"),
    pending: t("Pending", "Ожидает"),
    requested: t("Requested", "Запрошено"),
    processing: t("Processing", "Обрабатывается"),
    rejected: t("Rejected", "Отклонено"),
    suspended: t("Suspended", "Приостановлен"),
    blocked: t("Blocked", "Заблокирован"),
    disabled: t("Disabled", "Выключен"),
    redeemed: t("Redeemed", "Погашен"),
    expired: t("Expired", "Истёк"),
    used: t("Used", "Использован"),
    preparing: t("Preparing", "Подготавливается"),
    prepared: t("Prepared", "Подготовлен"),
    sending: t("Sending", "Отправляется"),
    sent: t("Sent", "Отправлен"),
    failed: t("Failed", "Ошибка"),
    canceled: t("Canceled", "Отменён"),
  };
  return <Badge tone={tone}>{label[s] ?? status}</Badge>;
}

// ---------------------------------------------------------------------------
// Table
// ---------------------------------------------------------------------------

export function Table({
  label,
  head,
  children,
}: {
  label: string;
  head: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="table-wrap" role="region" aria-label={label} tabIndex={0}>
      <table className="table">
        <thead>
          <tr>{head}</tr>
        </thead>
        <tbody>{children}</tbody>
      </table>
    </div>
  );
}

// ---------------------------------------------------------------------------
// States
// ---------------------------------------------------------------------------

export function Loading({ label }: { label?: string }) {
  const { t } = useI18n();
  return (
    <div className="loading-block" role="status" aria-live="polite">
      <span className="spinner" aria-hidden /> {label ?? t("Loading…", "Загрузка…")}
    </div>
  );
}

export function EmptyState({
  title,
  children,
}: {
  title: string;
  children?: ReactNode;
}) {
  return (
    <div className="empty">
      <div className="empty-title">{title}</div>
      {children ? <div style={{ fontSize: 14 }}>{children}</div> : null}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Copy button
// ---------------------------------------------------------------------------

export function CopyButton({ value, label }: { value: string; label?: string }) {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);
  return (
    <Button
      variant="ghost"
      type="button"
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(value);
          setCopied(true);
          setTimeout(() => setCopied(false), 1800);
        } catch {
          // clipboard unavailable — ignore
        }
      }}
    >
      {copied ? t("Copied ✓", "Скопировано ✓") : label ?? t("Copy", "Копировать")}
    </Button>
  );
}

// ---------------------------------------------------------------------------
// Brand
// ---------------------------------------------------------------------------

export function Brand() {
  return (
    <span className="brand" translate="no">
      <span>
        APIToken <em>Partners</em>
      </span>
    </span>
  );
}
