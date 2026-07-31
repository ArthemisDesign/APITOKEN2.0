"use client";

// OpenKeys-контекст engine-аккаунтов для таблицы «Аккаунты движка».
// Порт okDirectory()/okBadge()/okInfo() из admin-panel.js. В shared-модуле
// components/spend-stats-modal.tsx те же хелперы приватные, поэтому здесь —
// локальная копия (правило: shared-файлы не трогаем).
import type { ReactElement } from "react";
import { api } from "@/lib/api";
import { nanoMoney } from "@/lib/format";
import { isOpenkeys, type OkDirectoryRow } from "@/components/spend-stats-modal";

// Карта engineAccountId → метаданные ключа. Грузится лениво один раз за сессию
// вкладки; при сбое promise сбрасывается, чтобы повторить при следующем fetch.
let okDirPromise: Promise<Map<string, OkDirectoryRow>> | null = null;
export function okDirectory(): Promise<Map<string, OkDirectoryRow>> {
  okDirPromise ??= api<{ rows?: OkDirectoryRow[] }>("/openkeys-admin/lookup")
    .then((data) => new Map((data.rows ?? []).map((row) => [String(row.engineAccountId ?? ""), row])))
    .catch(() => {
      okDirPromise = null;
      return new Map<string, OkDirectoryRow>();
    });
  return okDirPromise;
}

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
