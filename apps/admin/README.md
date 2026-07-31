# @claude-api/admin — операционная админ-панель apiToken.sale

Next.js 16 / React 19 (App Router), порт 3700. Замена однофайловой панели
`crates/server/src/admin-panel.html` + `admin-panel.js` — тот же визуальный стиль,
те же эндпоинты, те же русские подписи.

## Модель безопасности

Секретов нет и не будет: браузер ходит по same-origin относительным путям
(`/overview`, `/admin/*`, `/openkeys-admin/*`, `/partner-admin/*`), аутентификацию
(forward_auth) и серверные ключи внедряет Caddy. Никаких env-файлов и
`NEXT_PUBLIC_*` ключей. Вся загрузка данных — в client-компонентах.

## Структура

- `src/lib/api.ts` — `api<T>(path, opts)` / `send<T>(path, method, body)`: typed fetch
  для same-origin JSON, `ApiError` со статусом и сообщением из тела.
- `src/lib/usePoll.ts` — `usePoll(key, fetcher, { interval })`: SWR-подобный опрос
  (дедупликация по key, пауза на скрытой вкладке, ревалидация на фокусе,
  stale-while-revalidate). `revalidateAll()` — кнопка ↻ в сайдбаре.
  Реестр ошибок источников: `subscribeErrors(listener)` / `getErrors()`
  (`PollError { key, message, dismissed }`), `dismissError(key)`,
  `refreshPoller(key)` — падает/снимается сам по итогу каждого fetch.
- `src/lib/toast.ts` — `toast(message, kind?)` (`kind: "ok" | "bad"`, по умолчанию
  `"ok"`; bad живёт 9 с и имеет ×, ok — 5 с) + `<Toaster/>` (смонтирован в layout).
- `src/lib/dialog.tsx` — `dialog(options): Promise<Record<string,string> | null>`,
  промис-замена prompt/confirm; `options: { title, message?, fields?: [{ name,
  label, type?, value? }], confirmLabel?, danger? }`. null — отмена (Esc/оверлей/
  «Отмена»), Enter сабмитит. `<DialogHost/>` смонтирован в layout.
- `src/lib/csv.ts` — `downloadCsv(filename, header, rows)` (`;`, RFC 4180, BOM),
  `buildCsv`/`csvCell` для тестов, `csvDate()` → `YYYY-MM-DD` для имени файла.
- `src/lib/sources.ts` — `sourceName(path)`: путь API → русская подпись источника
  (карта из admin-panel.js; query отрезается, неизвестный путь — как есть).
- `src/lib/format.ts` — форматтеры 1:1 из `admin-panel.js`: `nanoMoney` (целочисленные
  nanoUSD-строки через BigInt — единственный способ показывать деньги), `money`
  (легаси-поля коммерции в долларах, только отображение), `formatDate`, `ago`,
  `duration`, `ageText`, `ratio`, `plural`, `count`, `windowLabel`.
- `src/lib/nav.ts` — `NAV` (источник правды сайдбара), `isNavItemActive`.
- `src/lib/theme.ts` — `THEME_STORAGE_KEY` (`apitoken-admin-theme:v1`), `toggleTheme`.
- `src/lib/types.ts` — типы payload'ов бэкендов (все поля опциональны).
- `src/components/ui.tsx` — `PageHead`, `SectionHeader`, `CardGrid`, `StatCard`,
  `Banner`, `Dot`, `Pill`, `TableCard`, `EmptyRow`, `LoadingGrid`, `Modal`
  (Esc/оверлей закрывают, Tab-трап, возврат фокуса; `wide` для широких).
- `src/components/sidebar.tsx` — сайдбар с навигацией, обновлением и темой.
- `src/components/error-center.tsx` — `<ErrorCenter/>` (смонтирован в layout):
  красные карточки падающих источников с ↻/×, читает реестр ошибок usePoll.
- `src/components/spend-stats-modal.tsx` — модалка «Кто тратит» (`/spend-stats`,
  окна 24ч/7д/30д + произвольный диапазон, сводка charged vs real-API и OpenKeys).
  Подключение: `const { openSpendStats, spendStatsModal } = useSpendStatsModal()`,
  `openSpendStats` — в `StatCard.onClick`/`onClick` заголовка «потрачено»,
  `{spendStatsModal}` — в конец страницы. Типы `SpendStatsResponse`, `SpendPeriod`
  и хелпер `isOpenkeys` экспортированы.
- `src/app/page.tsx` — Сводка (эталонная страница; портируйте остальные по ней).
- `src/app/api/health/route.ts` — `GET /api/health` → `{"ok":true}` для watchdog.

## Конвенции страниц

1. Страница — `'use client'`, данные через `usePoll("page-key", load, { interval })`;
   все источники — одним `Promise.all` с `.catch(() => null)` на источник
   (деградация молчит, блоки показывают «—» / «источник недоступен»).
   Интервалы как в `admin-panel.js`: Сводка — 30 с, Подписки и Система — 10 с,
   остальные — без автоматического опроса (только фокус/кнопка ↻).
2. Русские подписи — дословно из `admin-panel.js`.
3. Деньги — только `nanoMoney` над integer-строками; JS number для сумм запрещён.
4. Пока данных нет (`data === undefined`) — `PageHead` + `LoadingGrid`.
5. Тяжёлые таблицы мемоизируйте (`React.memo`/`useMemo`); статичный JSX выносите
   из компонентов страниц.
6. Ошибки действий показывайте через `toast(..., "bad")`, успех — `toast(...)`;
   подтверждения и ввод — через `dialog()` (не `window.confirm/prompt`).
7. «Потрачено» в таблицах аккаунтов — кликабельно: `onClick={openSpendStats}`
   из `useSpendStatsModal()` + `title="Разбивка: сутки / 7 дней / 30 дней"`.
8. Экспорт таблиц — `downloadCsv(filename, header, rows)`, имя файла с датой
   через `csvDate()` (например `users-2026-07-31.csv`).

## Команды

```bash
pnpm dev          # next dev -p 3700
pnpm build
pnpm start        # next start -H 127.0.0.1 -p 3700
pnpm typecheck
pnpm test         # vitest run
```
