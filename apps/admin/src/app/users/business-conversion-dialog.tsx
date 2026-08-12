"use client";

import { useState } from "react";
import { Modal } from "@/components/ui";
import type { AdminUser } from "./users-lib";

export interface BusinessConversionTarget {
  user: AdminUser;
  initialDiscount: number;
}

export function parseBusinessDiscount(raw: string): number | null {
  const trimmed = raw.trim();
  if (!/^\d{1,2}$/.test(trimmed)) return null;
  const value = Number(trimmed);
  return value >= 0 && value <= 95 ? value : null;
}

function BusinessConversionDialogContent(props: {
  target: BusinessConversionTarget;
  submitting: boolean;
  onClose: () => void;
  onConfirm: (discountPercent: number) => void;
}) {
  const [discount, setDiscount] = useState(() => String(props.target.initialDiscount));
  const [error, setError] = useState<string | null>(null);
  const parsed = parseBusinessDiscount(discount);
  return (
    <form
      className="business-conversion-form"
      onSubmit={(event) => {
        event.preventDefault();
        if (parsed === null) {
          setError("Укажите целую скидку от 0 до 95%.");
          return;
        }
        props.onConfirm(parsed);
      }}
    >
      <div className="business-conversion-summary">
        <span className="business-discount-kicker">Новые условия</span>
        <b>B2B с индивидуальной базовой скидкой</b>
        <p>Скидка начнёт действовать после подтверждённой доставки цены в engine.</p>
      </div>
      <label className="business-conversion-field" htmlFor="business-conversion-discount">
        <span>Базовая скидка</span>
        <span className="business-percent-control">
          <input
            id="business-conversion-discount"
            name="discount"
            type="number"
            inputMode="numeric"
            min={0}
            max={95}
            step={1}
            autoComplete="off"
            value={discount}
            disabled={props.submitting}
            aria-describedby="business-conversion-hint"
            onChange={(event) => {
              setDiscount(event.target.value);
              setError(null);
            }}
          />
          <span>%</span>
        </span>
      </label>
      <p id="business-conversion-hint" className="business-conversion-hint">
        Провайдеры сначала наследуют эту ставку. Отдельные исключения можно настроить на странице B2B.
      </p>
      {error ? <div className="business-discount-error" role="alert">{error}</div> : null}
      <div className="dlg-actions">
        <button type="button" className="btn ghost" disabled={props.submitting} onClick={props.onClose}>
          Отмена
        </button>
        <button type="submit" className="btn" disabled={props.submitting}>
          {props.submitting ? "Переводим…" : "Перевести в B2B"}
        </button>
      </div>
    </form>
  );
}

export function BusinessConversionDialog(props: {
  target: BusinessConversionTarget | null;
  submitting: boolean;
  onClose: () => void;
  onConfirm: (discountPercent: number) => void;
}) {
  return (
    <Modal
      open={props.target !== null}
      title="Перевести клиента в B2B"
      message={props.target?.user.email ?? "Выберите договорную скидку для клиента."}
      onClose={props.onClose}
    >
      {props.target ? <BusinessConversionDialogContent {...props} target={props.target} /> : null}
    </Modal>
  );
}
