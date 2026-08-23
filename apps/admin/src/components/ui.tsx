"use client";

import Link from "next/link";
import { useEffect, useId, useLayoutEffect, useRef, type ReactNode } from "react";
import { stampTableLabels } from "@/lib/table-labels";

// UI-примитивы админки — визуальные аналоги хелперов admin-panel.js
// (card/pill/banner/empty/dialog). Классы совпадают с globals.css.
//
// Конвенции для страниц:
// - тяжёлые таблицы мемоизируйте (React.memo/useMemo) — realtime может обновить одну секцию;
// - статичный JSX выносите из компонентов страниц;
// - деньги форматируйте только через nanoMoney/money из @/lib/format.

export type Tone = "" | "ok" | "warn" | "bad";

export function PageHead(props: { title: ReactNode; sub?: ReactNode; badge?: ReactNode }) {
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
  const body = (
    <>
      <div className="label">{props.label}</div>
      <div className="value">{props.value}</div>
      <div className="hint">{props.hint}</div>
    </>
  );
  if (clickable) {
    return (
      <button type="button" className="card clickable" onClick={props.onClick} title={props.title}>
        {body}
      </button>
    );
  }
  return <div className="card" title={props.title}>{body}</div>;
}

export function Dot(props: { kind?: Tone | "off" }) {
  return <span className={"dot" + (props.kind ? " " + props.kind : "")} aria-hidden="true" />;
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

// Табличная карточка: на desktop — горизонтальный скролл; на телефоне CSS
// складывает строки в карточки, а заголовки колонок копируются в data-label.
export function TableCard(props: { children: ReactNode }) {
  const rootRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    const stamp = () => stampTableLabels(root);
    stamp();
    const observer = new MutationObserver(stamp);
    observer.observe(root, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, []);
  return (
    <div className="tcard" ref={rootRef}>
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

export function LoadingGrid(props: { count?: number; label?: string }) {
  return (
    <div className="loading-grid" role="status" aria-live="polite" aria-label={props.label ?? "Загрузка данных"}>
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
  const onCloseRef = useRef(props.onClose);
  const titleId = useId();
  const { open } = props;

  useEffect(() => {
    onCloseRef.current = props.onClose;
  }, [props.onClose]);

  useEffect(() => {
    if (!open) return;
    previousFocus.current = document.activeElement;
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    const dialog = dialogRef.current;
    const first = dialog?.querySelector<HTMLElement>("input:not([disabled]), textarea:not([disabled]), select:not([disabled]), button:not([disabled]), a[href]");
    first?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onCloseRef.current();
      }
      // Фокус-трап как в dialog()/spendStats() легаси: Tab циклит фокус внутри диалога.
      if (event.key === "Tab" && dialog) {
        const focusable = [...dialog.querySelectorAll<HTMLElement>(
          "button:not([disabled]),input:not([disabled]),textarea:not([disabled]),select:not([disabled]),a[href]",
        )].filter((item) => item.tabIndex !== -1);
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
      document.body.style.overflow = previousOverflow;
      if (previousFocus.current instanceof HTMLElement) previousFocus.current.focus();
    };
  }, [open]);

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
        aria-labelledby={titleId}
      >
        <h3 id={titleId}>{props.title}</h3>
        {props.message ? <p className="dlg-sub">{props.message}</p> : null}
        {props.children}
      </div>
    </div>
  );
}
