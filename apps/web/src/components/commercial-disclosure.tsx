"use client";

import Link from "next/link";
import { useI18n, type Language } from "./i18n-provider";
import { ComplianceNav } from "./compliance-pages";

const copy: Record<Language, {
  eyebrow: string; title: string; lead: string; cards: Array<{ label: string; title: string; text: string }>;
  exampleLabel: string; example: string; policy: string;
}> = {
  en: {
    eyebrow: "Clear billing", title: "What you buy and how you are charged", lead: "apiToken.sale sells prepaid access to supported Claude and GPT models through Anthropic-compatible and OpenAI-compatible API endpoints. There are no token packs or recurring subscription fees.",
    cards: [
      { label: "Product", title: "Prepaid API access", text: "Top up any whole USD amount. Your funded balance is available across every supported model and is consumed only by API usage." },
      { label: "Metering", title: "Official spend × paid share", text: "Each request is priced from the upstream model's official API rates. Your active B2C or B2B discount is then applied to calculate the platform balance charge." },
      { label: "Payment", title: "Final amount before checkout", text: "The checkout shows the payment provider, amount, currency, and status. Funds are credited after provider confirmation; unused paid balance is governed by the refund terms." },
    ],
    exampleLabel: "Example", example: "$100 official API usage × 50% paid after the flat 50% discount = $50 platform balance charge.",
    policy: "Payments, promotional credit, and refunds are described in the User Agreement.",
  },
  ru: {
    eyebrow: "Прозрачный биллинг", title: "Что вы покупаете и как рассчитывается списание", lead: "apiToken.sale продаёт предоплаченный доступ к поддерживаемым моделям Claude и GPT через Anthropic-совместимый и OpenAI-совместимый API endpoint. Фиксированных пакетов токенов и регулярной подписки нет.",
    cards: [
      { label: "Продукт", title: "Предоплаченный API-доступ", text: "Пополните баланс на любую целую сумму в долларах США. Средства доступны для всех поддерживаемых моделей и расходуются только при использовании API." },
      { label: "Расчёт", title: "Официальная стоимость × доля оплаты", text: "Каждый запрос рассчитывается по официальным ценам API выбранной модели. Затем применяется активная скидка B2C или B2B и определяется списание с баланса платформы." },
      { label: "Оплата", title: "Итоговая сумма до платежа", text: "На странице оплаты отображаются провайдер, сумма, валюта и статус. Средства зачисляются после подтверждения провайдера; возврат неиспользованного оплаченного баланса регулируется соглашением." },
    ],
    exampleLabel: "Пример", example: "$100 официального использования API × 50% к оплате после плоской скидки 50% = $50 списания с баланса платформы.",
    policy: "Платежи, приветственный бонус и возвраты описаны в Пользовательском соглашении.",
  },
};

export function CommercialDisclosure() {
  const { language } = useI18n();
  const content = copy[language];
  return <section className="commercial-disclosure">
    <div className="commercial-heading"><div><span className="tag">{content.eyebrow}</span><h2>{content.title}</h2></div><p>{content.lead}</p></div>
    <div className="commercial-grid">{content.cards.map((card) => <article key={card.label}><span>{card.label}</span><h3>{card.title}</h3><p>{card.text}</p></article>)}</div>
    <div className="commercial-example"><span>{content.exampleLabel}</span><strong>{content.example}</strong></div>
    <div className="commercial-policy"><p>{content.policy}</p><Link href="/terms">{language === "ru" ? "Условия оплаты и возврата →" : "Payment and refund terms →"}</Link></div>
    <ComplianceNav current="pricing" />
  </section>;
}
