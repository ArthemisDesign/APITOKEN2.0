"use client";

// Контекст ключа OpenKeys (метка, номинал, продавец, профиль) по engine-аккаунту
// для таблицы «Engine и service accounts». Порт okDirectory()/okInfo() из
// crates/server/src/admin-panel.js (строки 401-410): карта грузится лениво один
// раз за сессию вкладки; если портал недоступен — строки остаются без подписи.
// В shared components/spend-stats-modal.tsx эта пара приватная, поэтому для
// страницы она продублирована локально (тип OkDirectoryRow — оттуда).
import type { ReactElement } from "react";
import { api } from "@/lib/api";
import { nanoMoney } from "@/lib/format";
import type { OkDirectoryRow } from "@/components/spend-stats-modal";

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
