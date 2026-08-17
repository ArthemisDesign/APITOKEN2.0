import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude API в LiteLLM",
    h1: "Используйте Claude API в LiteLLM",
    description: "Маршрутизируйте LiteLLM к Claude через apiToken.sale: задайте api_base на router.apitoken.sale в litellm_params или конфиге прокси и платите за токены на 50% меньше.",
    keywords: ["claude api litellm", "litellm anthropic", "litellm claude", "litellm api_base anthropic", "litellm прокси claude"],
    dek: "LiteLLM говорит с Anthropic нативно и позволяет переопределить эндпоинт для каждой модели — одна строка конфига отправляет весь ваш Claude-трафик через шлюз со скидкой.",
    sections: [
      { h2: "Прямой вызов SDK", blocks: [
        { type: "code", code: `import litellm\n\nresponse = litellm.completion(\n    model="anthropic/claude-opus-4-8",\n    api_base="https://router.apitoken.sale",\n    api_key="sk-pool-•••",\n    messages=[{"role": "user", "content": "Hello"}],\n)` },
        { type: "note", text: "Новые аккаунты через Google или GitHub получают приветственный бонус $5 на баланс платформы — этого хватит, чтобы подключить инструменты и сделать реальные вызовы до первого пополнения." },
      ] },
      { h2: "Конфиг LiteLLM-прокси", blocks: [
        { type: "code", code: `# config.yaml\nmodel_list:\n  - model_name: claude-opus-4-8\n    litellm_params:\n      model: anthropic/claude-opus-4-8\n      api_base: https://router.apitoken.sale\n      api_key: sk-pool-•••` },
        { type: "p", text: "Запустите прокси с этим конфигом — и каждый клиент вашего LiteLLM-шлюза прозрачно использует дисконтный эндпоинт Claude. Удобно, когда много сервисов делят один слой маршрутизации." },
      ] },
      { h2: "Зачем вести Claude через LiteLLM сюда", blocks: [
        { type: "list", items: [
          "Одно место, чтобы переключить все сервисы на дешёвый эндпоинт.",
          "Тот же префикс anthropic/ у моделей и те же параметры, что вы уже используете.",
          "Расход по каждому ключу виден в панели apiToken.sale с детализацией до токенов.",
        ] },
      ] },
    ],
    faq: [
      { q: "Поддерживает ли LiteLLM кастомный api_base для Anthropic?", a: "Да — передайте api_base в litellm.completion() или в litellm_params конфига прокси, и LiteLLM будет слать Anthropic-запросы на https://router.apitoken.sale." },
      { q: "Сохраняется ли префикс anthropic/ у моделей?", a: "Да. Используйте anthropic/claude-opus-4-8 (или любую поддерживаемую модель), чтобы LiteLLM применял протокол Anthropic; меняются только эндпоинт и ключ." },
      { q: "Работает ли это для инструментов поверх LiteLLM?", a: "Да — всё, что маршрутизируется через LiteLLM (включая многие кодинг-агенты), наследует дисконтный эндпоинт из той же конфигурации." },
    ],
  };
