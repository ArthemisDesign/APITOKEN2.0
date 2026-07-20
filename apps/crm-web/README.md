# @claude-api/crm-web — APIToken CRM (crm.panel.apitoken.sale)

Отдельный внутренний продукт (bounded context «CRM & Parsing»). Next.js, порт **:3300**.
Вход гейтится Caddy basic_auth по СВОИМ учёткам (`Q_Sales`/`R_Sales`/`M_Sales`) — эти логины
дают доступ только к CRM; админы основной панели сюда не подключены. Карта и runbook —
`CRM_PORTAL.md` в корне репозитория.

```bash
pnpm --filter @claude-api/crm-web dev        # http://localhost:3300
pnpm --filter @claude-api/crm-web build
pnpm --filter @claude-api/crm-web typecheck
```
