import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "Kimi API 키 구매 방법",
    h1: "Kimi API 키 구매 방법",
    description: "Kimi K3와 Kimi for Coding용 선불 API 키를 구매해 Anthropic Messages 또는 OpenAI 호환 client에서 사용하고 공식 API 비용의 50%로 이용하세요.",
    keywords: ["kimi api 키 구매", "kimi api 키", "kimi k3 api", "kimi for coding api", "moonshot kimi api", "선불 kimi api"],
    dek: "Kimi는 unified router의 독립 모델 namespace로 제공됩니다. native Anthropic Messages route 또는 OpenAI 호환 client를 사용하며 Claude, GPT, Gemini와 같은 선불 잔액을 공유합니다.",
    sections: [
      { h2: "세 단계로 이용 시작", blocks: [
        { type: "steps", items: [
          "apiToken.sale 계정을 만들고 sk-pool 키를 발급합니다.",
          "카드 또는 암호화폐로 정수 달러 금액을 충전합니다. 사용자 측 별도 Kimi plan은 필요 없습니다.",
          "GET https://router.apitoken.sale/v1/models를 읽고 키의 live catalog가 노출하는 kimi/* ID를 선택합니다.",
        ] },
        sourceBlock("how-to-buy-kimi-api-key", 0, 1),
      ] },
      { h2: "Kimi route의 차이", blocks: [
        { type: "list", items: [
          "Kimi는 별도 provider namespace이지만 네 번째 wire format은 아닙니다. POST /v1/messages와 x-api-key 또는 unified OpenAI 호환 /v1 route를 사용합니다.",
          "공개 ID는 kimi/k3, kimi/kimi-for-coding 같은 subscription alias이며 내부 tariff 모델명이 아닙니다.",
          "K3에는 256K와 1M context 표기가 있고 Kimi for Coding에는 기본과 High Speed alias가 있습니다.",
          "모델 availability는 provider capacity와 key policy에 따라 달라질 수 있어 live /v1/models가 권위입니다.",
        ] },
      ] },
    ],
    faq: [
      { q: "Kimi에 별도 API 키가 필요한가요?", a: "아니요. 같은 sk-pool 키와 잔액이 Kimi와 다른 지원 provider를 포함합니다." },
      { q: "Kimi는 어떤 endpoint를 사용하나요?", a: "Anthropic Messages에는 https://router.apitoken.sale/v1/messages를, OpenAI 호환 client에는 /v1 Chat Completions를 사용합니다. 둘 다 공개 kimi/* ID를 받습니다." },
      { q: "왜 /v1/models를 먼저 확인해야 하나요?", a: "catalog가 key-scoped이므로 현재 routing과 pricing이 가능한 모델만 반환합니다." },
    ],
  };
