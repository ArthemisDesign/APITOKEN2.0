"use client";

// OpenKeys — порт 1:1 функций openkeys()/bindOpenkeys() из
// crates/server/src/admin-panel.js (строки 606-635): карточки сводки, фильтры
// (поиск + партия/использование/статус), таблица ключей с live-расходом,
// пейджинг по 50, включение/отключение ключа через dialog() + toast.
// Изменения списка приходят по OpenKeys SSE; интервальных и focus-запросов нет.
import {
  memo,
  startTransition,
  useCallback,
  useEffect,
  useState,
  type FormEvent,
} from "react";
import { send } from "@/lib/api";
import { useResources } from "@/lib/resources";
import { dialog } from "@/lib/dialog";
import { toast } from "@/lib/toast";
import { count, formatDate, nanoMoney } from "@/lib/format";
import {
  Banner,
  CardGrid,
  EmptyRow,
  LoadingGrid,
  PageHead,
  Pill,
  SectionHeader,
  StatCard,
  TableCard,
} from "@/components/ui";
import {
  buildKeysPath,
  clampOffset,
  okTypeLabel,
  PAGE_LIMIT,
  SELLER_ACTION_COPY,
  sellerActionToast,
  SELLERS_PATH,
  USAGE_LABELS,
  type OpenkeysQuery,
  type OpenkeysResponse,
  type OpenkeysRow,
  type OpenkeysSeller,
  type OpenkeysSellersResponse,
  type SellerAction,
  type SellerActionResult,
} from "./lib";

const EMPTY_QUERY: OpenkeysQuery = { offset: 0, q: "", batch: "", status: "", usage: "" };

// percentBar из admin-panel.js (строки 188-190): округление, пороги 95/70.
function PercentBar({ percent }: { percent: number }) {
  const value = Math.min(100, Math.max(0, Math.round(Number(percent) || 0)));
  const kind = value >= 95 ? "bad" : value >= 70 ? "warn" : "";
  return (
    <>
      <span className="bar">
        <i className={kind} style={{ width: value + "%" }} />
      </span>
      <span className="bar-label">{value}%</span>
    </>
  );
}

type KeyRowProps = {
  item: OpenkeysRow;
  pending: boolean;
  onToggle: (item: OpenkeysRow) => void;
};

// Строка каталога — мемоизирована: набор данных не меняется при наборе текста
// в поиске, поэтому ререндер страницы не пересобирает таблицу.
const KeyRow = memo(function KeyRow({ item, pending, onToggle }: KeyRowProps) {
  const usageState = item.usageState ?? "";
  return (
    <tr>
      <td className="left">
        <b className="mono">{item.keyMasked}</b>
        <div className="sub mono">
          {item.engineAccountId || item.id}
          {item.apiType ? " · " + okTypeLabel(item.apiType) : ""}
        </div>
      </td>
      <td className="left">
        <b>{item.batchLabel || "Без метки"}</b>
        <div className="sub mono">
          {item.batchId} · {item.createdBy}
        </div>
      </td>
      <td>
        <Pill kind={item.enabled ? "ok" : "bad"}>{item.enabled ? "активен" : "отключён"}</Pill>
        <div className="sub">{item.status === "delivered" ? "выдан" : "на складе"}</div>
      </td>
      <td>
        {item.usagePercent == null ? (
          <Pill kind="warn">{USAGE_LABELS[usageState] ?? "нет данных"}</Pill>
        ) : (
          <>
            <div>
              <PercentBar percent={Math.min(100, item.usagePercent)} />
            </div>
            <div className="sub">
              {item.usagePercent}% · {USAGE_LABELS[usageState] ?? usageState}
            </div>
          </>
        )}
      </td>
      <td>
        <b>{item.spentNano == null ? "—" : nanoMoney(item.spentNano)}</b>
      </td>
      <td>{item.remainingNano == null ? "—" : nanoMoney(item.remainingNano)}</td>
      <td>{nanoMoney(item.faceValueNano)}</td>
      <td>{formatDate(item.deliveredAt || item.createdAt, true)}</td>
      <td>
        <a className="link" href={item.viewUrl} target="_blank" rel="noreferrer">
          профиль ↗
        </a>
      </td>
      <td>
        <button
          type="button"
          className={"btn" + (item.enabled ? " bad" : "")}
          disabled={pending}
          onClick={() => onToggle(item)}
        >
          {item.enabled ? "отключить" : "включить"}
        </button>
      </td>
    </tr>
  );
});

type SellerRowProps = {
  seller: OpenkeysSeller;
  pending: boolean;
  onAction: (seller: OpenkeysSeller, action: SellerAction) => void;
};

// Строка админа-издателя: состав его выпуска и три массовых действия.
// «Пауза» и «снять паузу» обратимы, «аннулировать» — нет, поэтому кнопка красная
// и подтверждается вводом имени.
const SellerRow = memo(function SellerRow({ seller, pending, onAction }: SellerRowProps) {
  const keys = seller.keys ?? 0;
  const active = seller.active ?? 0;
  const disabled = seller.disabled ?? 0;
  return (
    <tr>
      <td className="left">
        <b className="mono">{seller.createdBy}</b>
        <div className="sub">{count(seller.batches ?? 0, "партия", "партии", "партий")}</div>
      </td>
      <td>
        <b>{keys}</b>
        <div className="sub">аннулировано {seller.revoked ?? 0}</div>
      </td>
      <td>
        <Pill kind={disabled ? "warn" : "ok"}>
          {active} / {disabled}
        </Pill>
        <div className="sub">активны / на паузе</div>
      </td>
      <td>
        {seller.delivered ?? 0}
        <div className="sub">на складе {seller.stock ?? 0}</div>
      </td>
      <td>{nanoMoney(seller.faceValueNano)}</td>
      <td>{formatDate(seller.lastIssuedAt ?? undefined, true)}</td>
      <td>
        <div className="actions wrap">
          <button
            type="button"
            className="btn"
            disabled={pending || active === 0}
            onClick={() => onAction(seller, "pause")}
          >
            пауза
          </button>
          <button
            type="button"
            className="btn ghost"
            disabled={pending || disabled === 0}
            onClick={() => onAction(seller, "resume")}
          >
            снять паузу
          </button>
          <button
            type="button"
            className="btn bad"
            disabled={pending || keys === 0}
            onClick={() => onAction(seller, "revoke")}
          >
            аннулировать
          </button>
        </div>
      </td>
    </tr>
  );
});

const SELLERS_HEAD = (
  <thead>
    <tr>
      <th className="left">админ</th>
      <th>ключи</th>
      <th>состояние</th>
      <th>выдано</th>
      <th>номинал</th>
      <th>последний выпуск</th>
      <th>массовые действия</th>
    </tr>
  </thead>
);

function Pager({
  offset,
  total,
  onPage,
}: {
  offset: number;
  total: number;
  onPage: (offset: number) => void;
}) {
  return (
    <div className="pager">
      <span>
        {total ? offset + 1 : 0}–{Math.min(offset + PAGE_LIMIT, total)} из {total}
      </span>
      <button
        type="button"
        className="btn ghost"
        disabled={offset <= 0}
        onClick={() => onPage(Math.max(0, offset - PAGE_LIMIT))}
      >
        Назад
      </button>
      <button
        type="button"
        className="btn ghost"
        disabled={offset + PAGE_LIMIT >= total}
        onClick={() => onPage(offset + PAGE_LIMIT)}
      >
        Дальше
      </button>
    </div>
  );
}

// Статичный JSX вынесен из компонента страницы (не пересоздаётся на ререндер).
const TABLE_HEAD = (
  <thead>
    <tr>
      <th className="left">ключ</th>
      <th className="left">метка · партия</th>
      <th>состояние</th>
      <th>использование</th>
      <th>потрачено</th>
      <th>остаток</th>
      <th>номинал</th>
      <th>событие</th>
      <th>профиль</th>
      <th>действие</th>
    </tr>
  </thead>
);

const STATUS_OPTIONS = (
  <>
    <option value="">любой статус</option>
    <option value="active">активные</option>
    <option value="disabled">отключённые</option>
  </>
);

const USAGE_OPTIONS = (
  <>
    <option value="">любое использование</option>
    <option value="unused">не использовались</option>
    <option value="used">используются</option>
    <option value="exhausted">исчерпаны</option>
    <option value="unavailable">нет live-данных</option>
  </>
);

const FOOTER = (
  <footer>
    Live-балансы читаются bounded batch-запросами, поэтому раздел не создаёт по два запроса на
    каждый ключ. Полные секреты здесь никогда не передаются. Выпуск и выдача партий остаются в{" "}
    <a className="link" href="https://openkeys.apitoken.sale/admin" target="_blank" rel="noreferrer">
      OpenKeys admin ↗
    </a>
    .
  </footer>
);

export default function OpenKeysPage() {
  const [query, setQuery] = useState<OpenkeysQuery>(EMPTY_QUERY);
  // Текст поиска — черновик до отправки формы (в легаси применяется по submit).
  const [draft, setDraft] = useState("");
  const [pendingId, setPendingId] = useState<string | null>(null);
  const [pendingSeller, setPendingSeller] = useState<string | null>(null);

  const path = buildKeysPath(query);
  const { data: result, isLoading } = useResources<{
    data: OpenkeysResponse;
    sellers: OpenkeysSellersResponse;
  }>({
    data: path,
    sellers: SELLERS_PATH,
  });
  const catalog = result.data;
  const catalogTotal = catalog?.total ?? 0;
  const effectiveOffset = clampOffset(query.offset, catalogTotal);

  useEffect(() => {
    if (catalog && effectiveOffset !== query.offset) {
      startTransition(() => setQuery((current) => ({ ...current, offset: effectiveOffset })));
    }
  }, [catalog, effectiveOffset, query.offset]);

  const applyFilters = (patch: Partial<OpenkeysQuery>) => {
    startTransition(() => setQuery((prev) => ({ ...prev, ...patch, offset: 0 })));
  };

  const submitFilters = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    applyFilters({ q: draft.trim() });
  };

  const goToPage = (offset: number) => {
    startTransition(() => setQuery((prev) => ({ ...prev, offset })));
  };

  // Включение/отключение ключа (bindOpenkeys в легаси): подтверждение dialog(),
  // затем POST /openkeys-admin/keys {id, enabled}, toast и обновление каталога.
  const toggleKey = useCallback(
    async (item: OpenkeysRow) => {
      const next = !item.enabled;
      const label = item.batchLabel || item.keyMasked || "ключ";
      const values = await dialog({
        title: (next ? "Включить " : "Отключить ") + label,
        message: next
          ? "Ключ снова начнёт принимать запросы."
          : "Запросы перестанут проходить, но ключ и история останутся в системе.",
        confirmLabel: next ? "Включить" : "Отключить",
        danger: !next,
      });
      if (!values) return;
      setPendingId(item.id ?? null);
      try {
        await send("/openkeys-admin/keys", "POST", { id: item.id, enabled: next });
        toast(next ? "Ключ включён." : "Ключ отключён.");
      } catch (error) {
        toast(error instanceof Error ? error.message : String(error), "bad");
      } finally {
        setPendingId(null);
      }
    },
    [],
  );

  // Массовое действие по админу-издателю: подтверждение (для аннулирования —
  // с вводом имени), затем POST /openkeys-admin/sellers. Сервер режет пачку по
  // потолку и возвращает остаток, поэтому итог показываем счётчиками, а не «готово».
  const runSellerAction = useCallback(
    async (seller: OpenkeysSeller, action: SellerAction) => {
      const createdBy = seller.createdBy;
      if (!createdBy) return;
      const copy = SELLER_ACTION_COPY[action];
      const values = await dialog({
        title: copy.title + " · " + createdBy,
        message: copy.message,
        confirmLabel: copy.confirmLabel,
        danger: copy.danger,
        fields: action === "revoke" ? [{ name: "confirm", label: "Имя админа" }] : undefined,
      });
      if (!values) return;
      if (action === "revoke" && values.confirm?.trim() !== createdBy) {
        toast("Имя не совпало — аннулирование отменено.", "bad");
        return;
      }
      setPendingSeller(createdBy);
      try {
        const result = await send<SellerActionResult>("/openkeys-admin/sellers", "POST", {
          createdBy,
          action,
        });
        toast(sellerActionToast(result), result.failed ? "bad" : "ok");
      } catch (error) {
        toast(error instanceof Error ? error.message : String(error), "bad");
      } finally {
        setPendingSeller(null);
      }
    },
    [],
  );

  if (isLoading && Object.values(result).every((value) => value === undefined)) {
    return (
      <>
        <PageHead title="OpenKeys" sub="данные загружаются, навигация уже доступна" />
        <LoadingGrid />
      </>
    );
  }

  const data = catalog;
  const sellers = result.sellers?.sellers;

  if (data === undefined) {
    return (
      <>
        <PageHead
          title="OpenKeys"
          sub="выпущенные через OpenKeys ключи, партии и live-использование"
          badge={<Pill kind="warn">degraded</Pill>}
        />
        <Banner kind="warn" title="Каталог OpenKeys временно недоступен">
          Остальные разделы продолжают работать. Источник проверяется автоматически.
        </Banner>
      </>
    );
  }

  const summary = data.summary ?? {};
  const total = catalogTotal;
  const offset = effectiveOffset;
  const rows = data.rows ?? [];
  const batches = data.batches ?? [];

  return (
    <>
      <PageHead
        title="OpenKeys"
        sub="отдельный реестр выпущенных ключей с фильтром по использованию"
        badge={
          <Pill kind={summary.disabled || summary.exhausted ? "warn" : "ok"}>
            {count(total, "ключ", "ключа", "ключей")}
          </Pill>
        }
      />

      {data.truncated ? (
        <Banner kind="warn" title="Каталог достиг защитного лимита">
          Фильтр применяется к 10 000 последним ключам. Уточните партию или поиск.
        </Banner>
      ) : null}

      <CardGrid>
        <StatCard label="ключи" value={total} hint="после текущих фильтров" />
        <StatCard
          label="активны / отключены"
          value={`${summary.active ?? 0} / ${summary.disabled ?? 0}`}
          hint="управление обратимо"
        />
        <StatCard
          label="используются"
          value={summary.used ?? 0}
          hint={`не трогали ${summary.unused ?? 0} · исчерпаны ${summary.exhausted ?? 0}`}
        />
        <StatCard
          label="потрачено"
          value={nanoMoney(summary.spentNano)}
          hint={"остаток " + nanoMoney(summary.remainingNano)}
        />
      </CardGrid>

      <SectionHeader
        title="Админы-издатели"
        sub="одно действие на весь выпуск конкретного админа — фильтры каталога на него не влияют"
      />

      {sellers === undefined ? (
        <Banner kind="warn" title="Список админов-издателей недоступен">
          Каталог ключей ниже продолжает работать; массовые действия временно недоступны.
        </Banner>
      ) : (
        <TableCard>
          <table>
            {SELLERS_HEAD}
            <tbody>
              {sellers.length ? (
                sellers.map((seller) => (
                  <SellerRow
                    key={seller.createdBy}
                    seller={seller}
                    pending={pendingSeller !== null && pendingSeller === seller.createdBy}
                    onAction={runSellerAction}
                  />
                ))
              ) : (
                <EmptyRow columns={7} />
              )}
            </tbody>
          </table>
        </TableCard>
      )}

      <SectionHeader title="Каталог ключей" sub="метка и партия всегда видны" />

      <form className="toolbar" onSubmit={submitFilters}>
        <label className="sr-only" htmlFor="ok-search">
          Поиск OpenKeys
        </label>
        <input
          id="ok-search"
          name="q"
          type="search"
          autoComplete="off"
          spellCheck={false}
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          placeholder="метка, маска ключа, acct_…, продавец или ID партии…"
        />
        <label className="sr-only" htmlFor="ok-batch">
          Партия
        </label>
        <select
          id="ok-batch"
          value={query.batch}
          onChange={(event) => applyFilters({ batch: event.target.value })}
        >
          <option value="">все партии</option>
          {batches.map((batch) => (
            <option key={batch.id} value={batch.id}>
              {(batch.label || "Без метки") +
                " · " +
                batch.createdBy +
                " · " +
                formatDate(batch.createdAt)}
            </option>
          ))}
        </select>
        <label className="sr-only" htmlFor="ok-usage">
          Использование
        </label>
        <select
          id="ok-usage"
          value={query.usage}
          onChange={(event) => applyFilters({ usage: event.target.value })}
        >
          {USAGE_OPTIONS}
        </select>
        <label className="sr-only" htmlFor="ok-status">
          Статус
        </label>
        <select
          id="ok-status"
          value={query.status}
          onChange={(event) => applyFilters({ status: event.target.value })}
        >
          {STATUS_OPTIONS}
        </select>
        <button className="btn" type="submit">
          Применить
        </button>
      </form>

      <TableCard>
        <table>
          {TABLE_HEAD}
          <tbody>
            {rows.length ? (
              rows.map((item) => (
                <KeyRow
                  key={item.id}
                  item={item}
                  pending={pendingId !== null && pendingId === item.id}
                  onToggle={toggleKey}
                />
              ))
            ) : (
              <EmptyRow columns={10} />
            )}
          </tbody>
        </table>
      </TableCard>

      <Pager offset={offset} total={total} onPage={goToPage} />

      {FOOTER}
    </>
  );
}
