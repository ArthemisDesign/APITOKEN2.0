import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude API в LangChain",
    h1: "Используйте Claude API в LangChain",
    description: "Подключите LangChain к Claude через apiToken.sale: направьте ChatAnthropic на router.apitoken.sale, оставьте те же ID моделей и платите за токены на 50% меньше.",
    keywords: ["claude api langchain", "langchain anthropic", "langchain claude", "chatanthropic base url", "langchain claude api ключ"],
    dek: "Интеграция Anthropic в LangChain принимает кастомный URL API, поэтому ваши цепочки и агенты работают с Claude через apiToken.sale после правки в две строки — те же модели, ниже цена за токен.",
    sections: [
      { h2: "Направьте ChatAnthropic на шлюз", blocks: [
        { type: "code", code: `from langchain_anthropic import ChatAnthropic\n\nllm = ChatAnthropic(\n    model="claude-opus-4-8",\n    anthropic_api_url="https://router.apitoken.sale",\n    anthropic_api_key="sk-pool-•••",\n)\nprint(llm.invoke("Hello").content)` },
        { type: "p", text: "Это вся интеграция: тот же пакет langchain-anthropic, те же ID моделей, тот же стриминг и вызов инструментов — меняются только эндпоинт и цена." },
        { type: "note", text: "Новые аккаунты через Google или GitHub получают приветственный бонус $5 на баланс платформы — этого хватит, чтобы подключить инструменты и сделать реальные вызовы до первого пополнения." },
      ] },
      { h2: "Или через переменные окружения", blocks: [
        { type: "code", code: `export ANTHROPIC_API_URL=https://router.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••` },
        { type: "p", text: "С заданным окружением ChatAnthropic подхватывает оба значения автоматически, поэтому в общих кодовых базах правки кода не нужны вовсе." },
      ] },
      { h2: "Что работает", blocks: [
        { type: "list", items: [
          "Цепочки, агенты и LangGraph-воркфлоу — протокол не меняется.",
          "Стриминг, вызов инструментов и структурированный вывод через стандартную интеграцию.",
          "Все поддерживаемые модели Claude (Opus, Sonnet, Haiku) на одном ключе и балансе.",
        ] },
      ] },
    ],
    faq: [
      { q: "Работает ли LangChain с кастомным эндпоинтом Claude API?", a: "Да. ChatAnthropic принимает anthropic_api_url (или переменную окружения ANTHROPIC_API_URL), поэтому можно направить его на https://router.apitoken.sale, не меняя больше ничего." },
      { q: "Работают ли агенты LangChain и вызов инструментов?", a: "Да — шлюз отдаёт стандартный Anthropic Messages API, поэтому вызов инструментов, стриминг и LangGraph-агенты ведут себя ровно как с официальным эндпоинтом." },
      { q: "Какие модели доступны из LangChain?", a: "Все поддерживаемые модели Claude — claude-opus-4-8, claude-sonnet-5, claude-haiku-4-5 и другие — на одном ключе и предоплаченном балансе." },
    ],
  };
