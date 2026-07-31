"use client";

// Тосты — порт toast() из admin-panel.js (строки 50-54): success авто-скрывается
// за 5 с, «bad» живёт 9 с и имеет кнопку закрытия; role = status/alert.
// Легаси аппендил div'ы в body; здесь — крошечный event-emitter и компонент
// <Toaster/> (смонтирован в src/app/layout.tsx), читающий стек через
// useSyncExternalStore. CSS-классы те же (.toast / .toast.bad / .icon-btn).
// Файл .ts без JSX: Toaster собран на createElement, чтобы логика оставалась
// тестируемой в node-окружении vitest.
import { createElement, useSyncExternalStore, type ReactElement } from "react";

export type ToastKind = "ok" | "bad";
export type ToastItem = { id: number; message: string; kind: ToastKind };

// Таймауты авто-скрытия — дословно из admin-panel.js.
export const TOAST_TIMEOUT_MS: Record<ToastKind, number> = { ok: 5000, bad: 9000 };

let items: ToastItem[] = [];
let nextId = 1;
const timers = new Map<number, ReturnType<typeof setTimeout>>();
const listeners = new Set<() => void>();

const EMPTY: ToastItem[] = [];

function emit(): void {
  for (const listener of listeners) listener();
}

// toast(message, kind?): kind "bad" — красный, с кнопкой × и таймаутом 9 с.
export function toast(message: string, kind: ToastKind = "ok"): void {
  const id = nextId++;
  items = [...items, { id, message, kind }];
  timers.set(
    id,
    setTimeout(() => dismissToast(id), TOAST_TIMEOUT_MS[kind]),
  );
  emit();
}

export function dismissToast(id: number): void {
  const timer = timers.get(id);
  if (timer !== undefined) {
    clearTimeout(timer);
    timers.delete(id);
  }
  if (!items.some((item) => item.id === id)) return;
  items = items.filter((item) => item.id !== id);
  emit();
}

export function subscribeToasts(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

// Стабильная ссылка: массив пересоздаётся только при изменении стека.
export function getToasts(): ToastItem[] {
  return items;
}

/** Только для тестов: сбросить стек и таймеры между сценариями. */
export function __clearToastsForTests(): void {
  for (const timer of timers.values()) clearTimeout(timer);
  timers.clear();
  items = [];
}

export function Toaster(): ReactElement | null {
  const current = useSyncExternalStore(subscribeToasts, getToasts, () => EMPTY);
  if (!current.length) return null;
  return createElement(
    "div",
    { className: "toasts" },
    current.map((item) =>
      createElement(
        "div",
        {
          key: item.id,
          className: "toast" + (item.kind === "bad" ? " bad" : ""),
          role: item.kind === "bad" ? "alert" : "status",
        },
        createElement("span", null, item.message),
        item.kind === "bad"
          ? createElement(
              "button",
              {
                type: "button",
                className: "icon-btn",
                "aria-label": "Закрыть сообщение",
                onClick: () => dismissToast(item.id),
              },
              "×",
            )
          : null,
      ),
    ),
  );
}
