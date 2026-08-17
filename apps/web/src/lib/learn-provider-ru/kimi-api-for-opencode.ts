import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "Как использовать Kimi API в OpenCode",
    h1: "Запускаем Kimi K3 и Kimi for Coding в OpenCode",
    description: "Подключите OpenCode к Kimi через apiToken.sale: router plugin, каталог для конкретного ключа, явные kimi/* IDs и один предоплаченный ключ.",
    keywords: ["kimi opencode", "kimi api opencode", "kimi k3 opencode", "настройка kimi for coding", "opencode custom provider", "kimi coding agent"],
    dek: "OpenCode умеет явно обращаться к namespace Kimi и читает живой каталог router. Это безопасный coding-agent вариант для переключения между K3 и Kimi for Coding без ручного списка лимитов.",
    sections: [
      { h2: "Установите и проверьте", blocks: [
        { type: "steps", items: [
          "Запустите installer apiToken.sale для OpenCode: он добавит router plugin и сохранит backup существующего config.",
          "Перезапустите OpenCode, чтобы plugin получил scoped-каталог моделей.",
          "Выполните один однозначный prompt с явной namespaced-моделью.",
        ] },
        sourceBlock("kimi-api-for-opencode", 0, 1),
      ] },
      { h2: "Безопасный выбор Kimi-модели", blocks: [
        { type: "list", items: [
          "apitoken/kimi/kimi-for-coding — экономичный coding default.",
          "apitoken/kimi/kimi-for-coding-highspeed — меньшая задержка за двойную токенную ставку.",
          "apitoken/kimi/k3-256k — K3 reasoning в меньшем context mode.",
          "apitoken/kimi/k3 — K3 с контекстом 1M, если он есть в каталоге.",
        ] },
        { type: "note", text: "Claude Code и Kimi Code тоже поддерживают Kimi, но настраиваются иначе: Claude Code требует закрепить каждый model tier, а Kimi Code — явный OpenAI-совместимый provider block." },
      ] },
    ],
    faq: [
      { q: "OpenCode поддерживает Kimi?", a: "Да. Router plugin apiToken.sale регистрирует живой Kimi namespace, а модель выбирается как apitoken/kimi/{model}." },
      { q: "Зачем plugin вместо статического списка?", a: "Он синхронизирует IDs, лимиты и доступность со scoped-каталогом ключа, поэтому retired или недоступные aliases не остаются в config." },
      { q: "Claude Code тоже работает с Kimi?", a: "Да, с другой настройкой. Направьте Claude Code на Anthropic endpoint и закрепите main, Opus, Sonnet, Haiku и subagent model variables на одном Kimi alias." },
    ],
  };
