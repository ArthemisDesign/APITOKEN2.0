import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Используйте Claude Code без подписки",
    h1: "Claude Code без плана за $200/месяц",
    description: "Запускайте Claude Code на балансе pay-as-you-go вместо ежемесячной подписки. Укажите ANTHROPIC_BASE_URL на router.apitoken.sale и платите только за то, что используете.",
    keywords: ["claude code без подписки", "claude code api ключ", "claude code pay as you go", "claude code дешево", "claude code без ежемесячного плана"],
    dek: "Claude Code не обязан означать фиксированный ежемесячный план. Направьте его на ключ API с предоплаченным балансом — и платите за токены, что идеально при неравномерном использовании или если просто хотите попробовать.",
    sections: [
      { h2: "Две переменные окружения", blocks: [
        { type: "code", code: `export ANTHROPIC_BASE_URL=https://router.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••\n\n# then just run\nclaude` },
        { type: "p", text: "Это всё изменение. Claude Code сохраняет все функции — он просто списывает с вашего предоплаченного баланса со скидкой вместо подписки." },
      ] },
      { h2: "Когда pay-as-you-go выгоднее", blocks: [
        { type: "list", items: [
          "Нерегулярное или всплесковое использование, где фиксированная месячная плата расточительна.",
          "Проба Claude Code до перехода на план.",
          "Несколько инструментов на одном балансе и одном ключе.",
        ] },
        { type: "note", text: "Новые аккаунты через Google или GitHub получают приветственный бонус $5 на баланс платформы; аккаунтам по email и паролю бонус не начисляется." },
      ] },
    ],
    faq: [
      { q: "Работает ли Claude Code с собственным ключом API?", a: "Да. Задайте ANTHROPIC_BASE_URL и ANTHROPIC_API_KEY — и Claude Code будет использовать ваш ключ и баланс напрямую." },
      { q: "Теряю ли я какие-то функции?", a: "Нет. Claude Code ведёт себя идентично; меняется только биллинг — с подписки на предоплату за токены." },
    ],
  };
