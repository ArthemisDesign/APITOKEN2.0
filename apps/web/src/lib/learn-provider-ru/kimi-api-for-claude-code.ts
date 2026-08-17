import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "Как использовать Kimi K3 в Claude Code",
    h1: "Kimi K3 и Kimi for Coding в Claude Code",
    description: "Настройте Claude Code для Kimi K3 или Kimi for Coding через apiToken.sale: закрепите все model tiers, сохраните контекст 1M и проверьте endpoint.",
    keywords: ["kimi claude code", "kimi k3 claude code", "kimi for coding claude code", "claude code custom model", "claude code kimi api", "k3 1m claude code"],
    dek: "Claude Code уже говорит на Anthropic Messages, поэтому может запускать Kimi напрямую. Надёжная настройка закрепляет каждый внутренний model tier на одном Kimi alias — иначе основная сессия работает, а subagents падают на унаследованной Claude-модели.",
    sections: [
      { h2: "Закрепите подключение и все model tiers", blocks: [
        sourceBlock("kimi-api-for-claude-code", 0, 0),
        { type: "p", text: "На Anthropic route используйте bare subscription alias. Для 256K-модели вроде k3-256k или kimi-for-coding оставьте tier pins, но уберите две переменные контекста 1M." },
      ] },
      { h2: "Проверяйте маршрут, а не самопрезентацию модели", blocks: [
        { type: "list", items: [
          "Откройте /status и убедитесь, что Anthropic base URL указывает на apiToken.sale.",
          "Не спрашивайте модель, кто она: system prompt Claude Code может заставить любой backend назвать себя Claude.",
          "Считайте none/off отключением reasoning K3, а не выбором другой модели. Live-матрица оставила такие turns на тарифе K3; kimi-k2.6 не является публично адресуемой моделью.",
          "Перед долгим закреплением alias проверьте GET /v1/models.",
        ] },
      ] },
    ],
    faq: [
      { q: "Claude Code поддерживает Kimi K3?", a: "Да. Укажите https://router.apitoken.sale и закрепите каждый model tier Claude Code на допущенном subscription alias Kimi." },
      { q: "Зачем закреплять все model variables Claude Code?", a: "Claude Code отдельно выбирает модели для основной сессии, tiers и subagents. Незакреплённый tier может унаследовать Claude ID и упасть только при запуске фонового пути." },
      { q: "Как сохранить полный контекст K3 1M в Claude Code?", a: "Используйте k3 или k3[1m] и установите CLAUDE_CODE_MAX_CONTEXT_TOKENS и CLAUDE_CODE_AUTO_COMPACT_WINDOW в 1048576." },
    ],
  };
