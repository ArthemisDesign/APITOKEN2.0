"use client";

// Центр ошибок — аналог #error-center + failures Map из admin-panel.js
// (строки 108-131): фиксированный список в правом верхнем углу с per-source
// статусом, кнопками «повторить» (↻ — рефetch конкретного URL) и «скрыть»
// (×). Источник данных — реестр общего request cache; он показывает только
// живые ресурсы, а отдельная recovery-версия отличает
// успешное восстановление от unmount/deactivate/dismiss.
import { useEffect, useRef, useSyncExternalStore, type ReactElement } from "react";
import {
  dismissError,
  getErrorRecoveryVersion,
  getErrors,
  refreshResource,
  subscribeErrors,
  type ResourceError,
} from "@/lib/resources";
import { sourceName } from "@/lib/sources";
import { toast } from "@/lib/toast";
import { Dot } from "@/components/ui";

const SERVER_ERRORS: ResourceError[] = [];

function ErrorActions({ failure }: { failure: ResourceError }): ReactElement {
  const name = sourceName(failure.key);
  return (
    <div className="error-actions">
      <button
        type="button"
        className="icon-btn"
        title={`Повторить: ${name}`}
        aria-label={`Повторить запрос: ${name}`}
        onClick={() => refreshResource(failure.key)}
      >
        ↻
      </button>
      <button
        type="button"
        className="icon-btn"
        title={`Скрыть: ${name}`}
        aria-label={`Скрыть сообщение: ${name}`}
        onClick={() => dismissError(failure.key)}
      >
        ×
      </button>
    </div>
  );
}

function ErrorMessage({ failure }: { failure: ResourceError }): ReactElement {
  return (
    <p>
      {failure.message}
      <br />
      {failure.hasData
        ? "Последние успешные данные остаются на экране. Повторите запрос или дождитесь события источника."
        : "Данных ещё нет. Повторите запрос или дождитесь восстановления источника."}
    </p>
  );
}

export function ErrorNotes({ errors }: { errors: ResourceError[] }): ReactElement | null {
  if (!errors.length) return null;
  if (errors.length > 3) {
    const withData = errors.filter((failure) => failure.hasData).length;
    return (
      <section className="error-note error-note-group" role="alert">
        <Dot kind="bad" />
        <div>
          <b>{errors.length} источников временно недоступны</b>
          <p>
            {withData
              ? `Для ${withData} источников показаны последние успешные данные.`
              : "Остальная навигация и доступные разделы продолжают работать."}
          </p>
          <details>
            <summary>Показать источники</summary>
            <ul>
              {errors.map((failure) => (
                <li key={failure.key}>
                  <span><strong>{sourceName(failure.key)}</strong><small>{failure.message}</small></span>
                  <ErrorActions failure={failure} />
                </li>
              ))}
            </ul>
          </details>
        </div>
      </section>
    );
  }
  return (
    <>
      {errors.map((failure) => (
        <section className="error-note" role="alert" key={failure.key}>
          <Dot kind="bad" />
          <div>
            <b>{sourceName(failure.key)} временно недоступен</b>
            <ErrorMessage failure={failure} />
          </div>
          <ErrorActions failure={failure} />
        </section>
      ))}
    </>
  );
}

export function ErrorCenter(): ReactElement | null {
  const errors = useSyncExternalStore(subscribeErrors, getErrors, () => SERVER_ERRORS);
  const recoveryVersion = useSyncExternalStore(subscribeErrors, getErrorRecoveryVersion, () => 0);
  const initialRecoveryVersion = useRef(recoveryVersion);

  useEffect(() => {
    if (recoveryVersion > initialRecoveryVersion.current) {
      initialRecoveryVersion.current = recoveryVersion;
      toast("Соединение восстановлено. Данные обновлены.");
    }
  }, [recoveryVersion]);

  if (!errors.length) return null;
  return (
    <div className="error-center" aria-live="polite">
      <ErrorNotes errors={errors} />
    </div>
  );
}
