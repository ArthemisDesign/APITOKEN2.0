import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "apiToken.sale против Portkey для Claude",
    h1: "apiToken.sale против Portkey",
    description: "Portkey — это AI-шлюз для маршрутизации и наблюдаемости на ваших собственных ключах провайдеров. apiToken.sale сам выдаёт ключ Claude и баланс со скидкой. Разбираемся, когда что использовать.",
    keywords: ["альтернатива portkey", "apitoken против portkey", "ai-шлюз claude", "portkey claude api", "шлюз claude api"],
    dek: "Эти инструменты решают разные задачи. Portkey стоит перед ключами провайдеров, которые у вас уже есть; apiToken.sale — это источник ключа Claude и скидочного баланса.",
    sections: [
      { h2: "Разные задачи", blocks: [
        { type: "p", text: "Portkey добавляет маршрутизацию, кэширование и наблюдаемость поверх ключей API, которые вы приносите сами. Он не продаёт доступ к Claude или скидку — за ним всё равно нужен пополненный аккаунт Anthropic." },
        { type: "p", text: "apiToken.sale — это источник ключа и баланса: нативный эндпоинт Anthropic по адресу https://router.apitoken.sale с единой скидкой 50% и без необходимости аккаунта Anthropic." },
      ] },
      { h2: "Их можно даже сочетать", blocks: [
        { type: "p", text: "Если вам нравится наблюдаемость Portkey, вы можете указать в нём ключ apiToken.sale как Anthropic-провайдера и получить скидку под капотом." },
        { type: "note", text: "Новые аккаунты через Google или GitHub получают приветственный бонус $5 на баланс платформы; аккаунтам по email и паролю бонус не начисляется." },
      ] },
    ],
    faq: [
      { q: "Даёт ли Portkey скидку на Claude?", a: "Нет — Portkey это шлюз поверх ключей, которые у вас уже есть. Скидочный ключ Claude и баланс даёт именно apiToken.sale." },
      { q: "Можно ли использовать их вместе?", a: "Да. Укажите ключ apiToken.sale как Anthropic-провайдера в Portkey, чтобы сохранить наблюдаемость и платить меньше." },
    ],
  };
