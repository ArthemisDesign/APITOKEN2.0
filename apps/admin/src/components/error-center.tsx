"use client";

// Центр ошибок — аналог #error-center + failures Map из admin-panel.js
// (строки 108-131): фиксированный список в правом верхнем углу с per-source
// статусом, кнопками «повторить» (↻ — рефetch конкретного poller'а) и «скрыть»
// (×). Источник данных — реестр ошибок usePoll (subscribeErrors/getErrors);
// он показывает только живые poller'ы, а отдельная recovery-версия отличает
// успешное восстановление от unmount/deactivate/dismiss.
import { useEffect, useRef, useSyncExternalStore, type ReactElement } from "react";
import {
  dismissError,
  getErrorRecoveryVersion,
  getErrors,
  refreshPoller,
  subscribeErrors,
  type PollError,
} from "@/lib/usePoll";
import { sourceName } from "@/lib/sources";
import { toast } from "@/lib/toast";
import { Dot } from "@/components/ui";

const SERVER_ERRORS: PollError[] = [];

export function ErrorCenter(): ReactElement | null {
  const errors = useSyncExternalStore(subscribeErrors, getErrors, () => SERVER_ERRORS);
  const recoveryVersion = useSyncExternalStore(subscribeErrors, getErrorRecoveryVersion, () => 0);
  const initialRecoveryVersion = useRef(recoveryVersion);

  useEffect(() => {
    if (recoveryVersion > initialRecoveryVersion.current) {
      initialRecoveryVersion.current = recoveryVersion;
      toast("Соединение восстановлено. Панель сейчас обновится.");
    }
  }, [recoveryVersion]);

  if (!errors.length) return null;
  return (
    <div className="error-center" aria-live="polite">
      {errors.map((failure) => (
        <section className="error-note" role="alert" key={failure.key}>
          <Dot kind="bad" />
          <div>
            <b>{sourceName(failure.key)} временно недоступен</b>
            <p>
              {failure.message}
              <br />
              Панель продолжает работать. Проверка восстановления выполняется автоматически.
            </p>
          </div>
          <div className="error-actions">
            <button
              type="button"
              className="icon-btn"
              title="Повторить"
              aria-label="Повторить запрос"
              onClick={() => refreshPoller(failure.key)}
            >
              ↻
            </button>
            <button
              type="button"
              className="icon-btn"
              title="Закрыть"
              aria-label="Закрыть сообщение"
              onClick={() => dismissError(failure.key)}
            >
              ×
            </button>
          </div>
        </section>
      ))}
    </div>
  );
}
