"use client";

import Link from "next/link";
import { useEffect, useRef, type ReactNode } from "react";

// UI-примитивы админки — визуальные аналоги хелперов admin-panel.js
// (card/pill/banner/empty/dialog). Классы совпадают с globals.css.
//
// Конвенции для страниц:
// - тяжёлые таблицы мемоизируйте (React.memo/useMemo) — рендер идёт каждый poll-тик;
// - статичный JSX выносите из компонентов страниц;
// - деньги форматируйте только через nanoMoney/money из @/lib/format.

export type Tone = "" | "ok" | "warn" | "bad";

export function PageHead(props: { title: string; sub?: ReactNode; badge?: ReactNode }) {
  return (
    <div className="page-head">
      <div>
        <h1>{props.title}</h1>
        {props.sub ? <p className="sub">{props.sub}</p> : null}
      </div>
      {props.badge ? <div className="badge">{props.badge}</div> : null}
    </div>
  );
}

export function SectionHeader(props: { title: string; sub?: string }) {
  return (
    <div className="sect">
      <h2>{props.title}</h2>
      {props.sub ? <span className="sect-sub">{props.sub}</span> : null}
    </div>
  );
}

export function CardGrid(props: { children: ReactNode }) {
  return <div className="cards">{props.children}</div>;
}

export function StatCard(props: {
  label: string;
  value: ReactNode;
  hint?: ReactNode;
  onClick?: () => void;
  title?: string;
}) {
  const clickable = Boolean(props.onClick);
  return (
    <div
      className={"card" + (clickable ? " clickable" : "")}
      onClick={props.onClick}
      title={props.title}
      role={clickable ? "button" : undefined}
    >
      <div className="label">{props.label}</div>
      <div className="value">{props.value}</div>
      <div className="hint">{props.hint}</div>
    </div>
  );
}

export function Dot(props: { kind?: Tone | "off" }) {
  return <span className={"dot" + (props.kind ? " " + props.kind : "")} />;
}

export function Banner(props: {
  kind?: Tone;
  dot?: Tone | "off";
  title: ReactNode;
  children?: ReactNode;
  href?: string;
}) {
  const kind = props.kind ?? "";
  const body = (
    <>
      <Dot kind={props.dot ?? kind} />
      <div>
        <b>{props.title}</b>
        {props.children ? <span className="muted">{props.children}</span> : null}
      </div>
    </>
  );
  if (props.href) {
    return (
      <Link className={"banner" + (kind ? " " + kind : "")} href={props.href} style={{ textDecoration: "none", color: "inherit" }}>
        {body}
      </Link>
    );
  }
  return <div className={"banner" + (kind ? " " + kind : "")}>{body}</div>;
}

export function Pill(props: { kind?: Tone | "info"; children: ReactNode }) {
  return <span className={"pill" + (props.kind ? " " + props.kind : "")}>{props.children}</span>;
}

// Табличная карточка: обёртка с горизонтальным скроллом. Разметку table
// пишет страница (th/td со sticky-заголовком — глобальные стили).
export function TableCard(props: { children: ReactNode }) {
  return (
    <div className="tcard">
      <div className="tscroll">{props.children}</div>
    </div>
  );
}

// Пустая строка таблицы — аналог empty(columns).
export function EmptyRow(props: { columns: number; text?: string }) {
  return (
    <tr>
      <td colSpan={props.columns} className="empty">
        {props.text ?? "данных нет"}
      </td>
    </tr>
  );
}

export function LoadingGrid(props: { count?: number }) {
  return (
    <div className="loading-grid" role="status" aria-label="Загрузка данных">
      {Array.from({ length: props.count ?? 8 }, (_, i) => (
        <div key={i} className="skeleton" />
      ))}
    </div>
  );
}

// Модалка — аналог dialog() из admin-panel.js. Управляемая (open/onClose),
// Escape и клик по оверлею закрывают, Tab циклит фокус внутри диалога,
// фокус возвращается триггеру.
export function Modal(props: {
  open: boolean;
  title: string;
  message?: string;
  wide?: boolean;
  onClose: () => void;
  children: ReactNode;
}) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const previousFocus = useRef<Element | null>(null);
  const { open, onClose } = props;

  useEffect(() => {
    if (!open) return;
    previousFocus.current = document.activeElement;
    const dialog = dialogRef.current;
    const first = dialog?.querySelector<HTMLElement>("input, button");
    first?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
      // Фокус-трап как в dialog()/spendStats() легаси: Tab циклит фокус внутри диалога.
      if (event.key === "Tab" && dialog) {
        const focusable = [...dialog.querySelectorAll<HTMLElement>("button,input,select,a[href]")].filter(
          (item) => !(item as HTMLButtonElement).disabled,
        );
        if (!focusable.length) return;
        const first = focusable[0];
        const last = focusable[focusable.length - 1];
        if (event.shiftKey && document.activeElement === first) {
          event.preventDefault();
          last.focus();
        } else if (!event.shiftKey && document.activeElement === last) {
          event.preventDefault();
          first.focus();
        }
      }
    };
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("keydown", onKey);
      if (previousFocus.current instanceof HTMLElement) previousFocus.current.focus();
    };
  }, [open, onClose]);

  if (!props.open) return null;
  return (
    <div
      className="overlay"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) props.onClose();
      }}
    >
      <div
        ref={dialogRef}
        className={"dialog" + (props.wide ? " wide" : "")}
        role="dialog"
        aria-modal="true"
        aria-label={props.title}
      >
        <h3>{props.title}</h3>
        {props.message ? <p className="dlg-sub">{props.message}</p> : null}
        {props.children}
      </div>
    </div>
  );
}
