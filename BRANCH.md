# Ветка `web/v2`

Ты на **ветке-владельце v2 клиентского сайта** — новая логика `apps/web` строится здесь,
прод (`apitoken.sale`, ветка Vercel production) не затрагивается вообще.

## Как деплоится (быстрый цикл, БЕЗ прод-протоколов)

`git push origin web/v2` → Vercel сам собирает preview-деплой (~1–2 мин). Никакого
agent-merge, watchdog, merge-lock — они существуют только для `master`. Стабильный адрес
последнего билда ветки:

```
https://claude-api-web-git-web-v2-mikhails-projects-d40f75bc.vercel.app
```

Красивый домен `v2.apitoken.sale` подключается один раз в дашборде Vercel
(Project claude-api-web → Settings → Domains → добавить `v2.apitoken.sale`,
Git Branch = `web/v2`) + DNS-запись у Namecheap, которую покажет Vercel
(нужна ЯВНАЯ запись для `v2`: сейчас его перехватывает wildcard `*.apitoken.sale → 84.32.48.2`).

ГОТЧА автора коммита: Vercel собирает preview только если автор коммита в команде Vercel.
Рабочий автор — `qqjamba@apitoken.sale` (см. память/`git config user.email`).

## Границы и инварианты (критично)

- Меняем ТОЛЬКО `apps/web/`. Движок, commerce (`apps/api`, `apps/worker`), `deploy/`,
  `systemd/` — не отсюда: они деплоятся watchdog-протоколом через `master`.
- Превью обязано оставаться noindex: `layout.tsx` и `robots.txt` уже отдают запрет при
  `VERCEL_ENV !== "production"`. Не ломать — staging-копия в выдаче убьёт SEO основного домена.
- ИЗВЕСТНОЕ ОГРАНИЧЕНИЕ: commerce-бэкенд пускает ровно один browser-origin
  (`PUBLIC_APP_BASE_URL` = https://apitoken.sale) — CORS + origin.guard на мутациях, поэтому
  живой логин с превью невозможен by design. РЕШЕНИЕ на этой ветке: превью-сборки
  (`VERCEL_ENV=preview`) работают в режиме «уже внутри» — `src/lib/preview-fixtures.ts`
  подменяет api-слой стейтфул-фикстурами (ключи создаются/переименовываются, промо и чекаут
  зачисляют баланс), `/` редиректит в `/dashboard`. В прод-сборке флаг
  `NEXT_PUBLIC_PREVIEW_FIXTURES` пуст — слой статически мёртв (см. `next.config.ts`).
  Локально: `NEXT_PUBLIC_PREVIEW_FIXTURES=1 npm run dev`.

## Проверка

```bash
cd apps/web && npm install
npx tsc --noEmit && npx vitest run && npm run build
AUDIT_SCOPE=dashboard node scripts/capture-site.mjs   # визуальный аудит (см. VISUAL_AUDIT.md)
```

## Перенос в прод (когда v2 готов)

Обычный путь: rebase на `origin/master` → merge в `master` через `./deploy/agent-merge.sh` —
Vercel production пересоберёт `apitoken.sale` из master. Старый прод-деплой остаётся в
истории деплоев Vercel для мгновенного отката (Instant Rollback).
