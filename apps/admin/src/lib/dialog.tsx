"use client";

// Промис-диалог — порт dialog() из admin-panel.js (строки 34-49): встроенная
// модалка вместо window.prompt/confirm, которые браузеры молча глушат.
// dialog({...}) → Promise<значения полей | null> (null — отмена: Esc, клик по
// оверлею, кнопка «Отмена»). Разметка и классы — через общий Modal
// (.overlay/.dialog/.dlg-*); фокус-трап и возврат фокуса — в Modal (ui.tsx),
// Enter сабмитит форму нативно.
// <DialogHost/> смонтирован в src/app/layout.tsx; стек запросов живёт снаружи
// React и читается через useSyncExternalStore — dialog() можно звать из любого
// обработчика без контекста.
import { useSyncExternalStore, type FormEvent, type ReactElement } from "react";
import { Modal } from "@/components/ui";
import { useI18n } from "@/lib/i18n";

export type DialogField = {
  name: string;
  label: string;
  /** type инпута, по умолчанию "text". */
  type?: string;
  /** Начальное значение поля. */
  value?: string;
};

export type DialogOptions = {
  title: string;
  message?: string;
  fields?: DialogField[];
  /** Подпись кнопки подтверждения, по умолчанию «Подтвердить». */
  confirmLabel?: string;
  /** Красная кнопка подтверждения (деструктивное действие). */
  danger?: boolean;
};

export type DialogRequest = DialogOptions & {
  id: number;
  resolve: (values: Record<string, string> | null) => void;
};

let requests: DialogRequest[] = [];
let nextId = 1;
const listeners = new Set<() => void>();

const EMPTY: readonly DialogRequest[] = [];

function emit(): void {
  for (const listener of listeners) listener();
}

// Открыть диалог. Промис резолвится объектом {имя поля: значение} при
// подтверждении или null при отмене.
export function dialog(options: DialogOptions): Promise<Record<string, string> | null> {
  return new Promise((resolve) => {
    requests = [...requests, { id: nextId++, ...options, resolve }];
    emit();
  });
}

// Завершить диалог: values — собранные поля (подтверждение), null — отмена.
// Экспортирован для DialogHost и юнит-тестов стора.
export function resolveDialog(id: number, values: Record<string, string> | null): void {
  const request = requests.find((item) => item.id === id);
  if (!request) return;
  requests = requests.filter((item) => item.id !== id);
  emit();
  request.resolve(values);
}

export function subscribeDialogs(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

// Стабильная ссылка: массив пересоздаётся только при изменении стека.
export function getDialogs(): readonly DialogRequest[] {
  return requests;
}

/** Только для тестов: сбросить стек между сценариями. */
export function __resetDialogsForTests(): void {
  requests = [];
}

function DialogView({ request }: { request: DialogRequest }): ReactElement {
  const { t } = useI18n();
  const cancel = () => resolveDialog(request.id, null);
  // Enter в любом инпуте сабмитит форму нативно — отдельный keydown не нужен.
  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const data = new FormData(event.currentTarget);
    const values: Record<string, string> = {};
    for (const field of request.fields ?? []) {
      values[field.name] = String(data.get(field.name) ?? "");
    }
    resolveDialog(request.id, values);
  };
  return (
    <Modal open title={request.title} message={request.message} onClose={cancel}>
      <form onSubmit={submit}>
        {(request.fields ?? []).map((field) => (
          <label className="dlg-label" key={field.name}>
            {field.label}
            <input
              type={field.type ?? "text"}
              name={field.name}
              defaultValue={field.value ?? ""}
              autoComplete="off"
              spellCheck={false}
            />
          </label>
        ))}
        <div className="dlg-actions">
          <button type="button" className="btn ghost" onClick={cancel}>
            {t("Cancel", "Отмена")}
          </button>
          <button type="submit" className={"btn" + (request.danger ? " bad" : "")}>
            {request.confirmLabel ?? t("Confirm", "Подтвердить")}
          </button>
        </div>
      </form>
    </Modal>
  );
}

export function DialogHost(): ReactElement {
  const current = useSyncExternalStore(subscribeDialogs, getDialogs, () => EMPTY);
  return (
    <>
      {current.map((request) => (
        <DialogView key={request.id} request={request} />
      ))}
    </>
  );
}
