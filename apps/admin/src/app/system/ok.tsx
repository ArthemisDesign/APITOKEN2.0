"use client";

// OpenKeys-контекст engine-аккаунтов для таблицы «Аккаунты движка».
// Порт okBadge()/okInfo() из admin-panel.js. Данные получает общий URL-store
// страницы; этот модуль содержит только представление.
import type { ReactElement } from "react";
import { nanoMoney } from "@/lib/format";
import { isOpenkeys, type OkDirectoryRow } from "@/components/spend-stats-modal";

const okTypeLabel = (type: string | undefined): string => (type === "openai" ? "OpenAI" : "Claude");

// Бейдж «OpenKeys» у handle, выпущенного через портал (okBadge из легаси).
export function OkBadge({ handle }: { handle: string | null | undefined }): ReactElement | null {
  if (!isOpenkeys(handle)) return null;
  return (
    <span className="okb" title="Выпущен через OpenKeys">
      OpenKeys
    </span>
  );
}

// Подпись под handle: метка, номинал, продавец, тип и ссылка на профиль (okInfo).
export function OkInfo({ meta }: { meta: OkDirectoryRow | undefined }): ReactElement | null {
  if (!meta) return null;
  return (
    <div className="sub">
      {meta.batchLabel || "Без метки"} · {nanoMoney(meta.faceValueNano)} · {meta.createdBy ?? "—"} ·{" "}
      {okTypeLabel(meta.apiType)}
      {meta.viewUrl ? (
        <>
          {" · "}
          <a className="link" href={meta.viewUrl} target="_blank" rel="noreferrer">
            профиль ↗
          </a>
        </>
      ) : null}
    </div>
  );
}
