"use client";

// Контекст ключа OpenKeys (метка, номинал, продавец, профиль) по engine-аккаунту
// для таблицы «Engine и service accounts». Порт okDirectory()/okInfo() из
// crates/server/src/admin-panel.js (строки 401-410). Данные получает URL-store
// страницы; компонент отвечает только за отображение строки справочника.
import type { ReactElement } from "react";
import { nanoMoney } from "@/lib/format";
import type { OkDirectoryRow } from "@/components/spend-stats-modal";

const okTypeLabel = (type: string | undefined): string => (type === "openai" ? "OpenAI" : "Claude");

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
