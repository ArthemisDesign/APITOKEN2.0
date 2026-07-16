"use client";

import Link from "next/link";
import { useI18n, type Language } from "./i18n-provider";

const SUPPORT_EMAIL = "apitokensale@gmail.com";
// apiToken Support — живой Telegram-бот первой линии (Claude Sonnet 5 через наш движок,
// эскалация на человека через Chatwoot). Реальный хэндл — @apitokensupportbot (SUPPORT.md).
const SUPPORT_TELEGRAM_HANDLE = "@apitokensupportbot";
const SUPPORT_TELEGRAM_URL = "https://t.me/apitokensupportbot";

type LegalSection = {
  title: string;
  paragraphs?: string[];
  bullets?: string[];
};

type LegalDocument = {
  eyebrow: string;
  title: string;
  summary: string;
  updated: string;
  notice: string;
  sections: LegalSection[];
};

const documents: Record<Language, { privacy: LegalDocument; terms: LegalDocument }> = {
  en: {
    privacy: {
      eyebrow: "Legal information",
      title: "Privacy Policy",
      summary: "How apiToken.sale collects, uses, shares, and protects information when you use the website, dashboard, payments, and API gateway.",
      updated: "Effective and last updated: July 15, 2026",
      notice: `Privacy requests are handled at ${SUPPORT_EMAIL}.`,
      sections: [
        {
          title: "1. Scope and operator",
          paragraphs: [
            `This Privacy Policy applies to apiToken.sale, its website, dashboard, authentication flows, payment flows, support, and API gateway (together, the “Service”). The Service operator is apiToken.sale. The official privacy and support contact is ${SUPPORT_EMAIL}.`,
            "By using the Service, you acknowledge this Policy. Where consent is required by applicable law, we will request it separately.",
          ],
        },
        {
          title: "2. Information we collect",
          bullets: [
            "Account information: email address, account identifier, verification status, customer type, and account settings.",
            "Authentication information: a securely hashed password for password accounts, or identifiers and profile data returned by Google or GitHub when you choose OAuth sign-in. We do not receive your Google or GitHub password.",
            "Technical and security information: IP address, browser and device details, timestamps, session identifiers, security events, and diagnostic logs.",
            "Website analytics: page path without query strings or fragments, timestamp, referrer, approximate location, device type, operating system, and browser. We receive this through Vercel Web Analytics as aggregated traffic statistics and do not link it to your account identity.",
            "API and billing metadata: API key identifier, selected model, request time, token and usage totals, official API cost, discount, balance charge, ledger reference, and request status.",
            "Payment information: top-up amount, currency, payment status, provider transaction identifiers, and webhook records. Full payment-card credentials are handled by the payment provider and are not stored by apiToken.sale.",
            "Support information: messages, attachments, account or order identifiers, and other information you voluntarily send to support.",
          ],
        },
        {
          title: "3. API request and response content",
          paragraphs: [
            "Prompt, message, tool, and response content necessarily passes through our infrastructure so the Service can route your request to the selected upstream model provider and return the result. We do not use this content for advertising or sell it.",
            "Operational records are designed around usage and billing metadata rather than prompt content. Content may still be processed temporarily in memory, transport buffers, security systems, or error diagnostics, and upstream model providers may process or retain it under their own terms. Do not submit secrets, regulated data, or personal data unless you have a lawful basis and appropriate safeguards.",
          ],
        },
        {
          title: "4. Why we use information",
          bullets: [
            "Create and secure accounts, authenticate sessions, and provide OAuth sign-in.",
            "Route API requests, calculate usage, apply the active discount, maintain balances, and show the request ledger.",
            "Create and reconcile payments, prevent duplicate credits, process refund requests, and maintain financial records.",
            "Detect abuse, fraud, compromised keys, attacks, and violations of the User Agreement.",
            "Provide support, send essential service notices, improve reliability, and comply with legal obligations.",
          ],
        },
        {
          title: "5. Legal grounds",
          paragraphs: [
            "Depending on applicable law, we process information to perform the contract with you, comply with legal and financial obligations, protect our legitimate interests in operating and securing the Service, and act on consent where consent is the appropriate basis. You may withdraw consent without affecting processing already performed, but some features cannot work without required data.",
          ],
        },
        {
          title: "6. When information is shared",
          paragraphs: [
            "We do not sell personal information. We share only what is reasonably necessary with infrastructure, hosting and analytics providers, authentication providers you select, payment providers shown at checkout, upstream model/API providers, security and monitoring vendors, professional advisers, and public authorities when lawfully required.",
            "A provider may act in another country and under its own privacy terms. We select providers for a defined operational purpose and limit access where reasonably possible.",
          ],
        },
        {
          title: "7. Cookies and local storage",
          paragraphs: [
            "The Service uses an essential secure, HttpOnly session cookie to keep you signed in. Language and theme preferences may be stored in your browser's local storage. These technologies are required for the requested functionality and are not used by us to sell advertising profiles.",
            "We use Vercel Web Analytics for anonymous, aggregated page-view statistics. It does not use third-party analytics cookies. Vercel creates a short-lived visitor hash from a request and discards the visitor-session identifier after 24 hours. apiToken.sale removes query strings and URL fragments before an analytics event is sent.",
          ],
        },
        {
          title: "8. Retention",
          paragraphs: [
            "We retain account information while the account is active and for a reasonable period afterward. Billing, payment, ledger, fraud-prevention, and security records may be kept longer where needed for accounting, dispute resolution, enforcement, backup integrity, or law. Support correspondence is retained as needed to resolve and document the request.",
            "When information is no longer required, we delete or anonymize it through normal operational cycles. Backup copies may remain until the relevant backup expires.",
          ],
        },
        {
          title: "9. Security",
          paragraphs: [
            "We use access controls, encrypted transport, secret hashing, restricted administrative access, and monitoring appropriate to the Service. No system is completely secure. You must keep your password, session, and API keys confidential and revoke a key immediately if you suspect exposure.",
          ],
        },
        {
          title: "10. Your choices and rights",
          paragraphs: [
            `Subject to applicable law, you may request access, correction, export, restriction, objection, or deletion of your personal information by emailing ${SUPPORT_EMAIL}. We may need to verify your identity. Some records cannot be deleted immediately where retention is legally required or necessary to resolve payments, fraud, or disputes.`,
            "You may disconnect OAuth access through the relevant provider and may stop using the Service at any time. You may also complain to the data-protection authority available in your jurisdiction.",
          ],
        },
        {
          title: "11. Age and third-party links",
          paragraphs: [
            "The Service is not directed to children. You must have legal capacity to enter this agreement and, where required, be at least 18 years old. Third-party sites and services linked from the Service are governed by their own policies.",
          ],
        },
        {
          title: "12. Changes and contact",
          paragraphs: [
            `We may update this Policy when the Service, providers, or legal requirements change. The current version and effective date will remain published on this page. Questions and privacy requests: ${SUPPORT_EMAIL}.`,
          ],
        },
      ],
    },
    terms: {
      eyebrow: "Legal information",
      title: "User Agreement",
      summary: "The rules for accounts, API access, prepaid balance, pricing, payments, refunds, and acceptable use of apiToken.sale.",
      updated: "Effective and last updated: July 15, 2026",
      notice: `Questions about these terms can be sent to ${SUPPORT_EMAIL}.`,
      sections: [
        {
          title: "1. Agreement and operator",
          paragraphs: [
            `This User Agreement (the “Agreement”) is between you and the operator of apiToken.sale (“apiToken.sale”, “we”, or “us”). The official contact is ${SUPPORT_EMAIL}. By registering, signing in, topping up, creating an API key, or using the Service, you accept this Agreement and the Privacy Policy. If you do not agree, do not use the Service.`,
          ],
        },
        {
          title: "2. What the Service provides",
          paragraphs: [
            "apiToken.sale provides prepaid access to supported third-party Claude models through a unified Anthropic-compatible API gateway, together with account, key, balance, usage-ledger, and support functions. We are an independent platform and are not affiliated with or endorsed by Anthropic, PBC.",
            "Model availability, limits, context windows, features, and upstream prices may change when upstream providers change their services. The current public model and pricing information is published on the Service.",
          ],
        },
        {
          title: "3. Eligibility and accounts",
          bullets: [
            "You must have legal capacity to enter a contract and provide accurate registration information.",
            "One person or organization may not create duplicate accounts to obtain repeated welcome credits or evade limits.",
            "You are responsible for activity through your account, sessions, and API keys. Notify support promptly about unauthorized access.",
            "Google and GitHub sign-in are optional third-party authentication methods and are also subject to the provider's terms.",
          ],
        },
        {
          title: "4. API keys and acceptable use",
          paragraphs: [
            "API keys are confidential credentials. Do not publish, transfer, or embed them in publicly distributed client code. Usage authenticated by your key is treated as your usage unless we determine that the Service caused an error.",
          ],
          bullets: [
            "Do not use the Service for illegal activity, malware, credential theft, fraud, harassment, exploitation, or infringement.",
            "Do not attack, scan, overload, reverse engineer, bypass limits, interfere with billing, or access another user's account or data.",
            "Do not resell or sublicense access unless a separate written B2B agreement expressly permits it.",
            "Comply with applicable law and the acceptable-use rules of upstream providers.",
          ],
        },
        {
          title: "5. Prices, tariffs, and metering",
          paragraphs: [
            "The Service uses a prepaid usage model, not fixed token packs. You choose a whole USD amount to add to your platform balance. A request's official API spend is calculated from the upstream model's published input, output, cache, and other applicable usage rates. We then apply your active discount and charge the result to your platform balance.",
            "Example: at a 60% discount you pay 40% of official API spend, so $100 of official API usage charges $40 from your platform balance. At an 80% discount you pay 20%, so $2,500 of official API usage charges $500. The request ledger is the billing record. Displayed estimates may be rounded, while billing uses the exact calculation.",
            "B2C discounts progress by calendar-month platform spend under the milestones published on the Pricing page. An achieved tier carries into the next month; missing its target reduces the tier by at most one level. B2B discounts are set under an individual invite-only agreement. Prices shown before payment and the active account discount control the transaction.",
          ],
        },
        {
          title: "6. Payments and promotional credits",
          paragraphs: [
            "Payments are processed by the payment provider displayed at checkout. A top-up is credited only after valid provider confirmation. Processing time, exchange rate, payment-network fee, and supported refund route may depend on that provider. Do not pay to an address or account not generated by the official checkout.",
            "A new eligible B2C account may receive promotional credit equal to $10 of usage at official API prices under the 60% starter discount, represented as $4 of platform balance. Promotional credit has no cash value, cannot be withdrawn or transferred, is limited to one eligible customer, and may be withheld or reversed in cases of duplicate accounts, fraud, or abuse.",
          ],
        },
        {
          title: "7. Refunds",
          paragraphs: [
            `You may request a refund of unused paid balance within 14 calendar days after the relevant payment by writing to ${SUPPORT_EMAIL} from the account email and including the order identifier. Consumed usage, promotional credit, and payment or network fees already incurred are not refundable.`,
            "Approved refunds are limited to the unused amount attributable to that payment and are returned to the original payment method where technically possible. Identity, ownership, and anti-fraud checks may be required. Provider processing times apply. Nothing in this section limits mandatory consumer rights or remedies for a Service failure that cannot legally be excluded.",
            "Please contact support before initiating a chargeback so we can investigate. Fraudulent or duplicate chargebacks may result in account suspension while the dispute is reviewed.",
          ],
        },
        {
          title: "8. Availability and upstream services",
          paragraphs: [
            "The Service is provided on an “as available” basis. We do not promise uninterrupted access, a specific model, output, latency, or result unless a separate written B2B agreement says otherwise. Requests can fail because of maintenance, capacity, networks, upstream providers, safety systems, or events beyond our reasonable control.",
            "Model outputs may be inaccurate, incomplete, or unsuitable. You are responsible for reviewing outputs and must not rely on them as professional, legal, medical, financial, or safety-critical advice without qualified human review.",
          ],
        },
        {
          title: "9. Suspension and termination",
          paragraphs: [
            "We may limit, suspend, or terminate access to protect users or the Service, respond to legal or provider requirements, investigate fraud or security incidents, collect an unpaid obligation, or address a material breach. Where reasonably possible, we will explain the restriction through the account email or support channel.",
            "You may stop using the Service and request account closure. Closure does not cancel completed usage, pending disputes, or records we must retain. Treatment of unused paid balance follows the Refunds section.",
          ],
        },
        {
          title: "10. Intellectual property",
          paragraphs: [
            "The Service software, design, documentation, and branding are protected by applicable intellectual-property laws. This Agreement gives you a limited, revocable, non-exclusive right to use the Service as intended. Rights in third-party models, software, and outputs are governed by the relevant provider terms and applicable law.",
          ],
        },
        {
          title: "11. Liability",
          paragraphs: [
            "To the maximum extent permitted by law, apiToken.sale is not liable for indirect, incidental, special, punitive, or consequential losses, lost profits, lost data, or decisions made from model output. Our aggregate liability connected with the Service will not exceed the paid amount you added to the Service during the three months before the event giving rise to the claim.",
            "These limitations do not apply where prohibited by law, including liability that cannot legally be limited. You remain responsible for your content, integrations, users, taxes, and compliance obligations.",
          ],
        },
        {
          title: "12. Changes, disputes, and contact",
          paragraphs: [
            "We may update this Agreement when the Service, pricing structure, providers, or legal requirements change. Material changes will be published on this permanent page with a new effective date. Changes do not retroactively alter completed charges. Continuing to use the Service after the effective date means you accept the updated Agreement.",
            `Please send billing, refund, and legal questions to ${SUPPORT_EMAIL}. The parties should first attempt to resolve a dispute through support. Applicable mandatory consumer law and the law determined by the operator's legal place of establishment continue to apply.`,
          ],
        },
      ],
    },
  },
  ru: {
    privacy: {
      eyebrow: "Правовая информация",
      title: "Политика конфиденциальности",
      summary: "Как apiToken.sale собирает, использует, передаёт и защищает информацию при использовании сайта, кабинета, платежей и API-шлюза.",
      updated: "Дата вступления в силу и последнего обновления: 15 июля 2026 года",
      notice: `Запросы по персональным данным принимаются по адресу ${SUPPORT_EMAIL}.`,
      sections: [
        {
          title: "1. Область действия и оператор",
          paragraphs: [
            `Настоящая Политика применяется к apiToken.sale, сайту, личному кабинету, авторизации, оплате, поддержке и API-шлюзу (совместно — «Сервис»). Оператор Сервиса — apiToken.sale. Официальный адрес для связи по вопросам конфиденциальности и поддержки: ${SUPPORT_EMAIL}.`,
            "Используя Сервис, вы подтверждаете ознакомление с Политикой. Если применимое законодательство требует отдельного согласия, мы запросим его отдельно.",
          ],
        },
        {
          title: "2. Какие данные мы собираем",
          bullets: [
            "Данные аккаунта: адрес электронной почты, идентификатор аккаунта, статус подтверждения, тип клиента и настройки.",
            "Данные авторизации: надёжно хешированный пароль для аккаунтов с паролем либо идентификаторы и данные профиля, полученные от Google или GitHub при выборе OAuth. Пароль от Google или GitHub нам не передаётся.",
            "Технические данные и данные безопасности: IP-адрес, сведения о браузере и устройстве, время событий, идентификаторы сессий, события безопасности и диагностические журналы.",
            "Веб-аналитика: путь страницы без параметров запроса и фрагментов, время, источник перехода, примерное местоположение, тип устройства, операционная система и браузер. Мы получаем эти сведения через Vercel Web Analytics в виде агрегированной статистики и не связываем их с личностью владельца аккаунта.",
            "Метаданные API и биллинга: идентификатор API-ключа, выбранная модель, время запроса, объём использования и токены, официальная стоимость API, скидка, списание с баланса, ссылка в журнале операций и статус запроса.",
            "Платёжные данные: сумма пополнения, валюта, статус, идентификаторы операции у провайдера и записи вебхуков. Полные данные банковской карты обрабатываются платёжным провайдером и не хранятся apiToken.sale.",
            "Данные поддержки: сообщения, вложения, идентификаторы аккаунта или заказа и иные сведения, которые вы добровольно отправляете.",
          ],
        },
        {
          title: "3. Содержимое запросов и ответов API",
          paragraphs: [
            "Промпты, сообщения, вызовы инструментов и ответы проходят через нашу инфраструктуру, поскольку Сервис должен направить запрос выбранному поставщику модели и вернуть результат. Мы не используем это содержимое для рекламы и не продаём его.",
            "Рабочие записи Сервиса ориентированы на метаданные использования и биллинга, а не на тексты промптов. Однако содержимое может временно обрабатываться в памяти, транспортных буферах, системах безопасности или диагностике ошибок, а поставщик модели может обрабатывать или хранить его по собственным условиям. Не передавайте секреты, регулируемые или персональные данные без законного основания и надлежащих мер защиты.",
          ],
        },
        {
          title: "4. Для чего используются данные",
          bullets: [
            "Создание и защита аккаунта, авторизация сессий и вход через OAuth.",
            "Маршрутизация API-запросов, расчёт использования, применение скидки, ведение баланса и журнала операций.",
            "Создание и сверка платежей, предотвращение повторных зачислений, рассмотрение возвратов и ведение финансовых записей.",
            "Выявление злоупотреблений, мошенничества, скомпрометированных ключей, атак и нарушений Пользовательского соглашения.",
            "Поддержка, обязательные сервисные уведомления, повышение надёжности и соблюдение требований закона.",
          ],
        },
        {
          title: "5. Правовые основания",
          paragraphs: [
            "В зависимости от применимого права мы обрабатываем данные для исполнения договора с вами, выполнения юридических и финансовых обязанностей, защиты законных интересов по работе и безопасности Сервиса, а также на основании согласия, когда оно является надлежащим основанием. Согласие можно отозвать без влияния на уже выполненную обработку, но без обязательных данных отдельные функции работать не смогут.",
          ],
        },
        {
          title: "6. Кому могут передаваться данные",
          paragraphs: [
            "Мы не продаём персональные данные. Необходимые сведения могут передаваться поставщикам инфраструктуры, хостинга и аналитики, выбранным вами провайдерам авторизации, платёжному провайдеру на странице оплаты, поставщикам моделей и API, сервисам безопасности и мониторинга, профессиональным консультантам и государственным органам при наличии законного требования.",
            "Поставщик может находиться в другой стране и применять собственные правила конфиденциальности. Мы привлекаем поставщиков для конкретной рабочей цели и по возможности ограничиваем доступ.",
          ],
        },
        {
          title: "7. Cookie и локальное хранилище",
          paragraphs: [
            "Сервис использует обязательный защищённый HttpOnly cookie сессии, чтобы сохранять вход в аккаунт. Язык и тема могут храниться в локальном хранилище браузера. Эти технологии нужны для запрошенных функций и не используются нами для продажи рекламных профилей.",
            "Мы используем Vercel Web Analytics для анонимной агрегированной статистики просмотров. Сервис аналитики не использует сторонние cookie. Vercel создаёт краткосрочный хеш посетителя из запроса и удаляет идентификатор сессии посетителя через 24 часа. apiToken.sale удаляет параметры запроса и фрагменты URL до отправки события аналитики.",
          ],
        },
        {
          title: "8. Срок хранения",
          paragraphs: [
            "Данные аккаунта хранятся, пока аккаунт активен, и разумный период после этого. Записи биллинга, платежей, журнала операций, предотвращения мошенничества и безопасности могут храниться дольше для бухгалтерии, разрешения споров, исполнения условий, целостности резервных копий или соблюдения закона. Переписка поддержки хранится столько, сколько нужно для решения и документирования обращения.",
            "Когда сведения больше не нужны, мы удаляем или обезличиваем их в рамках обычных рабочих циклов. Резервные копии могут сохраняться до окончания срока соответствующей копии.",
          ],
        },
        {
          title: "9. Безопасность",
          paragraphs: [
            "Мы применяем контроль доступа, шифрование при передаче, хеширование секретов, ограниченный административный доступ и мониторинг, соответствующие характеру Сервиса. Полностью безопасных систем не существует. Вы обязаны сохранять пароль, сессию и API-ключи в тайне и немедленно отозвать ключ при подозрении на утечку.",
          ],
        },
        {
          title: "10. Ваши права и возможности",
          paragraphs: [
            `С учётом применимого права вы можете запросить доступ, исправление, выгрузку, ограничение обработки, возражение или удаление персональных данных, написав на ${SUPPORT_EMAIL}. Для защиты аккаунта мы можем проверить вашу личность. Некоторые записи нельзя удалить немедленно, если хранение требуется законом либо необходимо для платежей, предотвращения мошенничества или споров.`,
            "Вы можете отключить OAuth-доступ у соответствующего провайдера и прекратить использование Сервиса в любой момент. Также вы вправе обратиться в доступный вам орган по защите данных.",
          ],
        },
        {
          title: "11. Возраст и сторонние ссылки",
          paragraphs: [
            "Сервис не предназначен для детей. Вы должны обладать дееспособностью для заключения договора и, если это требуется, быть не младше 18 лет. Для сторонних сайтов и сервисов действуют их собственные политики.",
          ],
        },
        {
          title: "12. Изменения и контакты",
          paragraphs: [
            `Мы можем обновлять Политику при изменении Сервиса, поставщиков или требований закона. Текущая версия и дата вступления в силу всегда публикуются на этой странице. Вопросы и запросы по данным: ${SUPPORT_EMAIL}.`,
          ],
        },
      ],
    },
    terms: {
      eyebrow: "Правовая информация",
      title: "Пользовательское соглашение",
      summary: "Правила работы с аккаунтом, API-доступом, предоплаченным балансом, тарифами, платежами, возвратами и допустимым использованием apiToken.sale.",
      updated: "Дата вступления в силу и последнего обновления: 15 июля 2026 года",
      notice: `Вопросы по условиям принимаются по адресу ${SUPPORT_EMAIL}.`,
      sections: [
        {
          title: "1. Принятие соглашения и оператор",
          paragraphs: [
            `Настоящее Пользовательское соглашение («Соглашение») заключено между вами и оператором apiToken.sale («apiToken.sale», «мы»). Официальный адрес для связи: ${SUPPORT_EMAIL}. Регистрируясь, входя в аккаунт, пополняя баланс, создавая API-ключ или используя Сервис, вы принимаете Соглашение и Политику конфиденциальности. Если вы не согласны, не используйте Сервис.`,
          ],
        },
        {
          title: "2. Что предоставляет Сервис",
          paragraphs: [
            "apiToken.sale предоставляет предоплаченный доступ к поддерживаемым сторонним моделям Claude через единый Anthropic-совместимый API-шлюз, а также функции аккаунта, ключей, баланса, журнала использования и поддержки. Мы являемся независимой платформой и не аффилированы с Anthropic, PBC и не одобрены этой компанией.",
            "Доступность моделей, лимиты, контекстные окна, функции и официальные цены могут меняться вслед за поставщиками. Актуальные модели и цены публикуются в Сервисе.",
          ],
        },
        {
          title: "3. Требования к пользователю и аккаунту",
          bullets: [
            "Вы должны обладать дееспособностью для заключения договора и указывать достоверные регистрационные данные.",
            "Нельзя создавать дублирующие аккаунты для повторного получения приветственного бонуса или обхода ограничений.",
            "Вы отвечаете за действия через аккаунт, сессии и API-ключи. О несанкционированном доступе нужно незамедлительно сообщить поддержке.",
            "Вход через Google и GitHub является необязательным сторонним способом авторизации и регулируется также условиями соответствующего провайдера.",
          ],
        },
        {
          title: "4. API-ключи и допустимое использование",
          paragraphs: [
            "API-ключ — конфиденциальная учётная информация. Не публикуйте, не передавайте и не встраивайте ключ в открыто распространяемый клиентский код. Использование, подтверждённое вашим ключом, считается вашим, если ошибка не была вызвана самим Сервисом.",
          ],
          bullets: [
            "Запрещено использовать Сервис для незаконной деятельности, вредоносного ПО, кражи учётных данных, мошенничества, преследования, эксплуатации или нарушения чужих прав.",
            "Запрещено атаковать, сканировать, перегружать или декомпилировать Сервис, обходить лимиты, вмешиваться в биллинг и получать доступ к чужому аккаунту или данным.",
            "Перепродажа и сублицензирование доступа запрещены без отдельного письменного B2B-соглашения.",
            "Необходимо соблюдать применимое право и правила допустимого использования поставщиков моделей.",
          ],
        },
        {
          title: "5. Цены, тарифы и учёт использования",
          paragraphs: [
            "Сервис работает по предоплате за использование, без фиксированных пакетов токенов. Вы выбираете целую сумму в долларах США для пополнения баланса платформы. Официальная стоимость запроса рассчитывается по опубликованным поставщиком ценам на входные, выходные, кэшированные и иные применимые единицы использования. Затем применяется активная скидка, а результат списывается с баланса платформы.",
            "Пример: при скидке 60% вы оплачиваете 40% официальной стоимости API, поэтому $100 официального использования списывают $40 с баланса платформы. При скидке 80% оплачивается 20%, поэтому $2 500 официального использования списывают $500. Журнал запросов является учётной записью биллинга. Публичные оценки могут округляться, но списание рассчитывается точно.",
            "Скидка B2C растёт по итогам расходов на платформе за календарный месяц согласно этапам на странице «Цены». Достигнутый уровень переносится на следующий месяц; при невыполнении цели уровень снижается не более чем на одну ступень. Скидка B2B устанавливается индивидуальным соглашением для приглашённого клиента. Для операции действуют цены, показанные до оплаты, и активная скидка аккаунта.",
          ],
        },
        {
          title: "6. Платежи и бонусы",
          paragraphs: [
            "Платёж проводит провайдер, указанный на странице оплаты. Пополнение зачисляется только после корректного подтверждения провайдера. Срок обработки, курс, комиссия сети и доступный способ возврата могут зависеть от провайдера. Не переводите средства на адрес или счёт, не созданный официальной страницей оплаты.",
            "Новый подходящий B2C-аккаунт может получить бонус, эквивалентный $10 использования по официальным ценам API при стартовой скидке 60%, то есть $4 баланса платформы. Бонус не имеет денежной стоимости, не выводится и не передаётся, предоставляется один раз одному подходящему клиенту и может быть отменён при дублях аккаунтов, мошенничестве или злоупотреблении.",
          ],
        },
        {
          title: "7. Возвраты",
          paragraphs: [
            `Запросить возврат неиспользованной части оплаченного баланса можно в течение 14 календарных дней после соответствующего платежа. Напишите с адреса аккаунта на ${SUPPORT_EMAIL} и укажите номер заказа. Уже использованные услуги, бонусы и фактически понесённые платёжные или сетевые комиссии не возвращаются.`,
            "Одобренный возврат ограничен неиспользованной суммой, относящейся к этому платежу, и по возможности выполняется исходным способом оплаты. Мы вправе проверить личность, владение аккаунтом и отсутствие мошенничества. Срок обработки зависит от провайдера. Этот раздел не ограничивает обязательные права потребителя и способы защиты при сбое Сервиса, которые нельзя исключить по закону.",
            "До оформления чарджбэка обратитесь в поддержку, чтобы мы могли провести проверку. Мошеннический или повторный чарджбэк может повлечь приостановку аккаунта на время спора.",
          ],
        },
        {
          title: "8. Доступность и сторонние поставщики",
          paragraphs: [
            "Сервис предоставляется по мере доступности. Мы не гарантируем непрерывную работу, конкретную модель, результат, задержку или итог, если иное не установлено отдельным письменным B2B-соглашением. Запросы могут завершаться ошибкой из-за обслуживания, ёмкости, сети, поставщиков, систем безопасности или обстоятельств вне нашего разумного контроля.",
            "Ответ модели может быть неточным, неполным или неподходящим. Вы обязаны проверять результат и не должны полагаться на него как на профессиональную, юридическую, медицинскую, финансовую или критически важную рекомендацию без проверки квалифицированным специалистом.",
          ],
        },
        {
          title: "9. Ограничение и прекращение доступа",
          paragraphs: [
            "Мы вправе ограничить, приостановить или прекратить доступ для защиты пользователей и Сервиса, исполнения требований закона или поставщика, расследования мошенничества и инцидента безопасности, взыскания задолженности либо устранения существенного нарушения. По возможности причина будет сообщена на email аккаунта или через поддержку.",
            "Вы можете прекратить использование и запросить закрытие аккаунта. Закрытие не отменяет уже оказанные услуги, незавершённые споры и записи, которые мы обязаны хранить. Неиспользованный оплаченный баланс регулируется разделом о возвратах.",
          ],
        },
        {
          title: "10. Интеллектуальная собственность",
          paragraphs: [
            "Программное обеспечение, дизайн, документация и обозначения Сервиса защищены применимым правом. Соглашение предоставляет ограниченное, отзывное и неисключительное право использовать Сервис по назначению. Права на сторонние модели, программы и результаты регулируются условиями соответствующего поставщика и законом.",
          ],
        },
        {
          title: "11. Ответственность",
          paragraphs: [
            "В максимально допустимой законом степени apiToken.sale не отвечает за косвенные, случайные, специальные, штрафные и последующие убытки, упущенную прибыль, потерю данных и решения, принятые на основе ответа модели. Совокупная ответственность по Сервису ограничена оплаченной суммой, зачисленной вами за три месяца до события, ставшего основанием требования.",
            "Ограничения не действуют там, где они запрещены законом, включая ответственность, которую нельзя ограничить. Вы отвечаете за свой контент, интеграции, пользователей, налоги и выполнение обязательных требований.",
          ],
        },
        {
          title: "12. Изменения, споры и контакты",
          paragraphs: [
            "Мы можем обновлять Соглашение при изменении Сервиса, структуры цен, поставщиков или требований закона. Существенные изменения публикуются на этой постоянной странице с новой датой вступления в силу. Изменения не пересчитывают завершённые списания задним числом. Продолжение использования после даты вступления в силу означает принятие новой редакции.",
            `Вопросы по биллингу, возвратам и правовым условиям направляйте на ${SUPPORT_EMAIL}. Сначала стороны должны попытаться разрешить спор через поддержку. Продолжают действовать обязательные нормы защиты потребителей и право, определяемое юридическим местом регистрации оператора.`,
          ],
        },
      ],
    },
  },
};

const supportCopy: Record<Language, {
  eyebrow: string; title: string; summary: string; emailLabel: string; emailHelp: string;
  write: string; official: string; topicsTitle: string; topics: string[]; includeTitle: string;
  include: string[]; securityTitle: string; security: string; refundsTitle: string; refunds: string;
  bot: { poweredBy: string; name: string; desc: string; points: string[]; cta: string; availability: string; handleLabel: string };
  how: { title: string; steps: { h: string; p: string }[] };
  emailKicker: string;
}> = {
  en: {
    eyebrow: "Customer support", title: "apiToken Support", summary: "Get help fast. Our AI assistant answers instantly in Telegram, and hands you over to a real person whenever a case needs targeted, human attention.",
    emailLabel: "Official support email", emailHelp: "For account-specific paperwork or if you can't use Telegram — email is a permanent channel for B2C and B2B.", write: "Write an email", emailKicker: "Prefer email?",
    official: "Public groups, comments, and direct messages from unrelated accounts are not official apiToken.sale support channels. The only support bot is @apitokensupportbot.",
    topicsTitle: "What the assistant can do", topics: ["Sign-up, top-up, and issuing or revoking API keys", "Pointing an SDK or tool at the endpoint and fixing request errors", "Explaining pricing tiers, discounts, and how to read your usage", "Diagnosing base URL, x-api-key header, model id, and balance issues"],
    includeTitle: "For a faster answer", include: ["The email address used for your account", "The exact error text or model id you're calling", "A clear description, the approximate time, and screenshots without secrets"],
    securityTitle: "Protect your account", security: "Never send a password, complete API key, OAuth token, payment secret, seed phrase, or full card details. Neither the bot nor a human operator will ever ask you to disclose them.",
    refundsTitle: "Money & refunds", refunds: "Anything about a specific charge, missing top-up, or refund is handed straight to a human operator. Send refund requests within 14 calendar days of payment with the order identifier; eligibility is governed by the User Agreement.",
    bot: {
      poweredBy: "AI first line · Claude Sonnet 5",
      name: "apiToken Support",
      desc: "Chat with our support bot on Telegram. It knows the product inside out and resolves most questions on the spot — sign-up, keys, endpoint setup, request errors, pricing and usage — then quietly hands you to a human when your case needs one.",
      points: ["Instant answers, 24/7, in your language", "Setup, keys, endpoints, and request errors", "Pricing tiers, discounts, and reading your usage", "Seamless hand-off to a person for money & account-specific help"],
      cta: "Open in Telegram",
      availability: "Live now · replies in seconds · a human joins the same chat when needed",
      handleLabel: "Support bot",
    },
    how: {
      title: "How support works",
      steps: [
        { h: "Message the bot", p: "Open @apitokensupportbot in Telegram and describe your problem in plain words. Attach screenshots if they help — never secrets." },
        { h: "AI answers instantly", p: "Claude Sonnet 5 replies in seconds: how to connect, why a request failed, how billing and discounts work, and exactly what to do next." },
        { h: "A human steps in", p: "Anything about your specific balance, a payment, or a complex case is escalated to a real operator — in the same chat, with the full context already there." },
      ],
    },
  },
  ru: {
    eyebrow: "Поддержка клиентов", title: "apiToken Support", summary: "Помощь без ожидания. ИИ-ассистент мгновенно отвечает в Telegram и передаёт вас живому человеку, как только вопрос требует точечного, ручного разбора.",
    emailLabel: "Официальная почта поддержки", emailHelp: "Для документов по аккаунту или если Telegram недоступен — почта остаётся постоянным каналом для B2C и B2B.", write: "Написать письмо", emailKicker: "Удобнее почтой?",
    official: "Публичные группы, комментарии и сообщения от посторонних аккаунтов не являются официальными каналами поддержки apiToken.sale. Единственный бот поддержки — @apitokensupportbot.",
    topicsTitle: "С чем поможет ассистент", topics: ["Регистрация, пополнение, выпуск и отзыв API-ключей", "Настройка SDK или инструмента на endpoint и разбор ошибок запросов", "Объяснение тарифов, скидок и как читать своё использование", "Диагностика base URL, заголовка x-api-key, id модели и баланса"],
    includeTitle: "Чтобы ответить быстрее", include: ["Email, на который зарегистрирован аккаунт", "Точный текст ошибки или id модели, к которой обращаетесь", "Понятное описание, примерное время и скриншоты без секретных данных"],
    securityTitle: "Защитите аккаунт", security: "Никогда не отправляйте пароль, полный API-ключ, OAuth-токен, платёжный секрет, seed-фразу или полные данные карты. Ни бот, ни оператор никогда не попросят их раскрыть.",
    refundsTitle: "Деньги и возвраты", refunds: "Всё, что касается конкретного списания, непришедшего пополнения или возврата, сразу передаётся живому оператору. Запрос возврата — в течение 14 календарных дней после оплаты с номером заказа; условия определяются Пользовательским соглашением.",
    bot: {
      poweredBy: "ИИ первой линии · Claude Sonnet 5",
      name: "apiToken Support",
      desc: "Напишите нашему боту поддержки в Telegram. Он знает продукт до мелочей и решает большинство вопросов на месте — регистрация, ключи, настройка endpoint, ошибки запросов, тарифы и использование — а когда нужно, незаметно передаёт диалог человеку.",
      points: ["Мгновенные ответы 24/7 на вашем языке", "Настройка, ключи, endpoint и ошибки запросов", "Тарифы, скидки и как читать своё использование", "Плавная передача человеку по деньгам и вопросам аккаунта"],
      cta: "Открыть в Telegram",
      availability: "Уже работает · отвечает за секунды · человек подключается в тот же чат при необходимости",
      handleLabel: "Бот поддержки",
    },
    how: {
      title: "Как работает поддержка",
      steps: [
        { h: "Напишите боту", p: "Откройте @apitokensupportbot в Telegram и опишите проблему простыми словами. Можно приложить скриншоты — но без секретов." },
        { h: "ИИ отвечает сразу", p: "Claude Sonnet 5 отвечает за секунды: как подключиться, почему упал запрос, как работают биллинг и скидки и что делать дальше." },
        { h: "Подключается человек", p: "Вопросы про конкретный баланс, платёж или сложный случай эскалируются живому оператору — в тот же чат, где уже есть весь контекст." },
      ],
    },
  },
};

export function ComplianceNav({ current }: { current: "privacy" | "terms" | "support" | "pricing" }) {
  const { language } = useI18n();
  const labels = language === "ru"
    ? { privacy: "Конфиденциальность", terms: "Соглашение", support: "Поддержка", pricing: "Цены и тарифы" }
    : { privacy: "Privacy Policy", terms: "User Agreement", support: "Support", pricing: "Prices & tariffs" };
  const links = [{ id: "privacy", href: "/privacy" }, { id: "terms", href: "/terms" }, { id: "support", href: "/support" }, { id: "pricing", href: "/plans" }] as const;
  return <nav className="compliance-nav" aria-label={language === "ru" ? "Правовая и коммерческая информация" : "Legal and commercial information"}>
    {links.map((link) => <Link className={current === link.id ? "active" : ""} href={link.href} key={link.id}>{labels[link.id]}</Link>)}
  </nav>;
}

export function LegalPage({ kind }: { kind: "privacy" | "terms" }) {
  const { language } = useI18n();
  const document = documents[language][kind];
  return <>
    <div className="page-hero legal-hero"><div className="wrap"><span className="eyebrow">{document.eyebrow}</span><h1>{document.title}</h1><p>{document.summary}</p><ComplianceNav current={kind} /></div></div>
    <section className="borderless legal-section"><div className="wrap legal-layout">
      <aside className="legal-aside"><span>{document.updated}</span><strong>{document.notice}</strong><a href={`mailto:${SUPPORT_EMAIL}`}>{SUPPORT_EMAIL}</a></aside>
      <article className="legal-document">{document.sections.map((section) => <section className="legal-document-section" key={section.title}><h2>{section.title}</h2>{section.paragraphs?.map((paragraph) => <p key={paragraph}>{paragraph}</p>)}{section.bullets && <ul>{section.bullets.map((bullet) => <li key={bullet}>{bullet}</li>)}</ul>}</section>)}</article>
    </div></section>
  </>;
}

export function SupportPage() {
  const { language } = useI18n();
  const copy = supportCopy[language];
  return <>
    <div className="page-hero legal-hero"><div className="wrap"><span className="eyebrow">{copy.eyebrow}</span><h1>{copy.title}</h1><p>{copy.summary}</p><ComplianceNav current="support" /></div></div>
    <section className="borderless support-section"><div className="wrap">

      <div className="support-bot">
        <div className="support-bot-glow" aria-hidden="true" />
        <div className="support-bot-main">
          <div className="support-bot-head">
            <span className="support-bot-av" aria-hidden="true">
              <svg viewBox="0 0 24 24" width="26" height="26" fill="currentColor" aria-hidden="true"><path d="M22 2 2.5 10.6c-.9.4-.9 1.6.1 1.9l4.6 1.4 1.8 5.6c.3.9 1.4 1.1 2 .4l2.5-2.8 4.7 3.4c.8.6 2 .1 2.2-.9L23.9 3.3C24.1 2.3 23 1.5 22 2ZM9 13.6l8.3-5.7-6.4 6.9-.1 3.4L9 13.6Z"/></svg>
            </span>
            <div>
              <span className="support-bot-tag">{copy.bot.poweredBy}</span>
              <h2>{copy.bot.name}</h2>
            </div>
          </div>
          <p className="support-bot-desc">{copy.bot.desc}</p>
          <ul className="support-bot-points">{copy.bot.points.map((item) => <li key={item}>{item}</li>)}</ul>
          <div className="support-bot-actions">
            <a className="btn btn-primary support-bot-cta" href={SUPPORT_TELEGRAM_URL} target="_blank" rel="noreferrer">{copy.bot.cta}</a>
            <a className="support-bot-handle" href={SUPPORT_TELEGRAM_URL} target="_blank" rel="noreferrer"><span>{copy.bot.handleLabel}</span>{SUPPORT_TELEGRAM_HANDLE}</a>
          </div>
          <span className="support-bot-avail"><i className="support-bot-dot" aria-hidden="true" />{copy.bot.availability}</span>
        </div>
      </div>

      <div className="support-how">
        <h2 className="support-how-title">{copy.how.title}</h2>
        <div className="support-flow">{copy.how.steps.map((step, index) => <article className="support-step" key={step.h}>
          <span className="n">{String(index + 1).padStart(2, "0")}</span>
          <h3>{step.h}</h3>
          <p>{step.p}</p>
        </article>)}</div>
      </div>

      <div className="support-grid">
        <div className="support-card"><h2>{copy.topicsTitle}</h2><ul>{copy.topics.map((item) => <li key={item}>{item}</li>)}</ul></div>
        <div className="support-card"><h2>{copy.includeTitle}</h2><ul>{copy.include.map((item) => <li key={item}>{item}</li>)}</ul></div>
        <div className="support-card"><h2>{copy.securityTitle}</h2><p>{copy.security}</p></div>
        <div className="support-card"><h2>{copy.refundsTitle}</h2><p>{copy.refunds}</p><Link href="/terms">{language === "ru" ? "Открыть Пользовательское соглашение →" : "Open the User Agreement →"}</Link></div>
      </div>

      <div className="support-email-card"><div><span>{copy.emailKicker}</span><a href={`mailto:${SUPPORT_EMAIL}`}>{SUPPORT_EMAIL}</a><p>{copy.emailHelp}</p></div><a className="btn btn-ghost" href={`mailto:${SUPPORT_EMAIL}`}>{copy.write}</a></div>
      <p className="support-official">{copy.official}</p>
    </div></section>
  </>;
}
