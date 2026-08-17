import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Цены Kimi API: как считается стоимость",
    h1: "Цены Kimi API: cache hit, miss, output и скорость",
    description: "Разбор цен Kimi K3, Kimi for Coding и High Speed: cache-hit, cache-miss, output, mapping aliases и скидка apiToken.sale 50%.",
    keywords: ["цены kimi api", "цена kimi k3", "цена kimi for coding", "стоимость токенов kimi", "цена kimi k2.7 code", "дешевый kimi api"],
    dek: "Kimi публикует отдельные ставки cache hit, cache miss и output. apiToken.sale тарифицирует фактически обслужившую модель, не смешивает usage-компоненты и применяет скидку 50%.",
    sections: [
      { h2: "Официальные ставки за публичными aliases", blocks: [
        { type: "table", headers: ["Публичный alias", "Официально hit / miss / output", "После скидки 50%"], rows: [
          ["kimi/k3 · k3-256k · k3[1m]", "$0.30 / $3 / $15", "$0.15 / $1.50 / $7.50"],
          ["kimi/kimi-for-coding", "$0.19 / $0.95 / $4", "$0.095 / $0.475 / $2"],
          ["kimi/kimi-for-coding-highspeed", "$0.38 / $1.90 / $8", "$0.19 / $0.95 / $4"],
        ] },
        { type: "p", text: "Все цены указаны за 1 млн токенов. Кэширование автоматическое. Отдельной цены cache write нет, поэтому новый cached-токен считается miss, а не бесплатным четвёртым компонентом." },
      ] },
      { h2: "Как контролировать расходы", blocks: [
        { type: "list", items: [
          "Kimi for Coding — самый экономичный общий coding-вариант.",
          "High Speed берите, только когда меньшая задержка оправдывает удвоенные токенные ставки.",
          "Используйте k3-256k вместо 1M-варианта, когда большой контекст не нужен.",
          "Задайте lifetime spending limit ключа и смотрите settled usage в дашборде.",
        ] },
        { type: "note", text: "Reasoning-токены входят в output и оплачиваются по output rate, а не второй отдельной строкой." },
      ] },
    ],
    faq: [
      { q: "Сколько стоит Kimi for Coding?", a: "Официально $0.19 за 1 млн cache-hit, $0.95 за cache-miss и $4 за output; apiToken.sale списывает половину." },
      { q: "Зачем разные цены cache hit и miss?", a: "Kimi автоматически кэширует повторный контекст. Terminal usage показывает, какие input-токены пришли из кэша, и каждый компонент получает свою ставку." },
      { q: "High Speed дороже?", a: "Да. Его cache-hit, cache-miss и output ставки ровно вдвое выше базового Kimi for Coding." },
    ],
  };
