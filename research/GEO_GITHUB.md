# GEO/SEO через GitHub — рабочая инструкция (аккаунт apitokensale-admin)

> Как использовать выделенный GitHub-аккаунт для GEO/SEO и внешних OSS-интеграций.
> Значений токенов в репозитории НЕТ и быть не должно: документация хранит только локаторы
> macOS Keychain и безопасный способ временно загрузить credential в процесс.

## 1. Аккаунт и доступ

| Что | Значение |
|---|---|
| Аккаунт | `apitokensale-admin` (создан 2026-07-17) |
| Публичные репозитории | `apitokensale-admin/apitoken.sale` и форки для OSS-интеграций |
| Основной `gh` credential | fine-grained PAT в системном keyring; не заменять |
| Classic PAT | macOS Keychain: service `apitoken-sale/github-classic-pat`, account `apitokensale-admin` |
| Локальный env-loader | `~/.config/apitoken-sale/github.env`, права `600`; содержит только чтение из Keychain |
| Проверка 2026-07-28 | GitHub API вернул HTTP 200 и аккаунт `apitokensale-admin` |
| Срок текущего classic PAT | до 2026-08-27 11:45:35 UTC |

### Зачем два credential

Fine-grained PAT подходит для наших репозиториев, но GitHub не разрешает ему создавать PR в
чужих публичных репозиториях. Для внешних PR нужен classic PAT как минимум со scope
`public_repo`. Текущий classic PAT фактически имеет более широкий scope `repo` и дополнительные
административные scopes. Он работоспособен, но избыточен: при ближайшей плановой ротации заменить
его токеном только с минимально необходимыми правами.

Classic PAT сохранён отдельно и не меняет активную сессию `gh`. Переменная `GH_TOKEN`, загруженная
в конкретный процесс, временно имеет приоритет над keyring. Поэтому loader не подключается в
`.zshrc`, `.profile`, LaunchAgent или глобальную конфигурацию.

### Безопасная загрузка для внешнего PR

Работать в отдельном subshell, чтобы `GH_TOKEN` исчез после завершения:

```bash
(
  . "$HOME/.config/apitoken-sale/github.env"
  gh auth status
  # gh pr create/view/comment ...
)
```

Файл `github.env` не содержит PAT. Его единственная задача — прочитать секрет командой
`security find-generic-password` из Keychain и экспортировать `GH_TOKEN`. На другой машине этого
credential нет: его нельзя копировать из Keychain в репозиторий, CI, сервер или документацию.

### Готча: два GitHub-аккаунта на этой машине

Глобальный git credential helper (`osxkeychain`) может отдавать credential **Q666Q666Q**. Push в
форк `apitokensale-admin/*` с ним падает с 403. В каждом отдельном клоне GEO/интеграции нужно
локально направлять Git credential на `gh`, не меняя глобальную конфигурацию:

```bash
git config --local --replace-all credential.helper ''
git config --local --add credential.helper '!gh auth git-credential'
git config user.name apitokensale-admin
git config user.email apitokensale-admin@users.noreply.github.com
```

Основной монорепозиторий использует собственный credential и workflow
`deploy/agent-merge.sh`; classic PAT для внешних PR не должен его заменять.

### Правила безопасности

- Никогда не записывать значение PAT в `.env`, tracked/untracked файлы репозитория, git config,
  remote URL, команды, логи, отчёты, PR, issues или комментарии.
- Не печатать `GH_TOKEN`, не включать shell tracing (`set -x`) при загруженном loader.
- Не добавлять loader в глобальный shell startup: classic PAT нужен только отдельным процессам.
- Не использовать PAT для действий от имени основного аккаунта `Q666Q666Q` или для production
  deployment этого монорепозитория.
- Если значение попало в публичный файл, лог или GitHub, немедленно отозвать токен, создать новый
  с минимальными scopes и обновить ту же запись Keychain без изменения документации.
- После ротации проверить аккаунт, срок, scopes, создание PR и права файла loader (`600`).

## 2. Зачем GitHub для GEO

LLM-краулеры индексируют GitHub; README популярных репозиториев попадают в RAG-выдачу
ИИ-поисковиков. Цель: когда пользователь ищет Anthropic-compatible API provider или способ
подключить Claude API, модель знает и уместно упоминает `apitoken.sale`. GitHub дополняет уже
сделанное на сайте (`llms.txt`, `/docs/learn`, allowlist в `robots.txt`).

Наружное позиционирование всегда одно: **Anthropic-compatible API provider**. В публичных
репозиториях и PR нельзя описывать внутреннюю механику, источники квот, ротацию, fingerprint или
authbot.

## 3. Портфель собственных репозиториев

Приоритет сверху вниз:

1. **`claude-api-docs`** — Markdown-зеркало пользовательских статей `/docs/learn`, инструкции по
   подключению клиентов к Anthropic-compatible endpoint, GitHub Pages.
2. **`awesome-claude-api`** — курируемый список SDK, клиентов, провайдеров и гайдов с прозрачными
   правилами включения.
3. **`claude-api-examples`** — проверяемые Python/TypeScript-примеры: обычный запрос, streaming,
   tool use и популярные клиенты.
4. **`claude-endpoint-benchmark`** — отдельный последующий этап: проверка совместимости и latency
   Anthropic-compatible endpoints.

## 4. Требования к каждому собственному репозиторию

- README начинается с answer-first описания, затем даёт проверяемый Quick Start и FAQ.
- Ссылки на `apitoken.sale` уместны в README, description и поле Website, без спама.
- Topics отражают реальный скоуп: `claude`, `claude-api`, `anthropic`, `anthropic-api`, `llm`.
- LICENSE, description, `llms.txt` и примеры должны быть актуальными.
- Содержательные обновления важнее искусственной активности; звёзды и трафик не накручивать.
- Контент можно брать из публичных пользовательских страниц продукта, но внутренности движка не
  публиковать.

## 5. Внешние OSS-интеграции

Постоянная хронология, тесты, PR, ревью и backlink-проверки ведутся в
`research/INTEGRATIONS_TRACKER.md`. Для каждой цели сохранять upstream SHA, правила контрибьюции,
принятые аналоги, минимальный diff, результаты тестов, PR URL, CI/CLA/review и дату появления
ссылки в default branch и публичной документации.

Classic PAT используется только для действий, которые fine-grained PAT действительно не может
выполнить, прежде всего для создания и сопровождения PR из форка в чужой публичный репозиторий.

## 6. Метрики импакта

- Звёзды, форки и GitHub traffic собственных репозиториев.
- Статус внешних PR и наличие `apitoken.sale` в merged diff, default branch и публичных доках.
- GitHub code search по `apitoken.sale` после каждого мержа или релиза.
- Referral-трафик с GitHub в аналитике сайта.
- Периодическая ручная проверка релевантных ответов ChatGPT, Claude и Perplexity без накрутки.
