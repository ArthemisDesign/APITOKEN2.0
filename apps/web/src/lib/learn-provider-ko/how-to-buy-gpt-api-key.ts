import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "GPT API 키 구매 방법",
    h1: "GPT API 키 구매 방법",
    description: "선불 잔액과 카드·암호화폐 결제로 GPT API 키를 구매하고 OpenAI 호환 endpoint에서 GPT-5.6, GPT-5.5, GPT Image 2를 공식 비용의 50%로 사용하세요.",
    keywords: ["gpt api 키 구매", "gpt api 키", "openai api 키 구매", "gpt-5.6 api", "openai 호환 api", "선불 gpt api"],
    dek: "apiToken.sale 키 하나로 별도 OpenAI Platform 계정 없이 GPT 카탈로그를 사용할 수 있습니다. 잔액을 충전하고 OpenAI 호환 endpoint를 설정하면 모든 요청의 공식 비용에서 50%가 할인됩니다.",
    sections: [
      { h2: "세 단계로 GPT 키 받기", blocks: [
        { type: "steps", items: [
          "apiToken.sale 계정을 만들고 대시보드에서 키를 발급합니다.",
          "고정 상품이나 월 약정 없이 카드 또는 암호화폐로 정수 달러 금액을 충전합니다.",
          "base URL을 https://router.apitoken.sale/v1로 설정하고 Authorization: Bearer를 사용한 뒤 GET /v1/models에서 모델을 선택합니다.",
        ] },
        sourceBlock("how-to-buy-gpt-api-key", 0, 1),
      ] },
      { h2: "키에 포함되는 기능", blocks: [
        { type: "list", items: [
          "증분 SSE streaming을 지원하는 Responses와 Chat Completions.",
          "GPT-5.6 Sol, Terra, Luna, 이전 GPT tier와 별도 GPT Image 2 route.",
          "같은 키와 잔액으로 지원되는 Claude, Gemini, Kimi 모델 사용.",
          "모든 요청의 공식 provider 비용에 적용되는 고정 50% B2C 할인.",
        ] },
        { type: "note", text: "키는 서버 환경 변수에 보관하세요. GPT는 Authorization: Bearer를 사용하며 x-api-key와 x-goog-api-key는 각각 Anthropic과 Gemini 프로토콜용입니다." },
      ] },
    ],
    faq: [
      { q: "OpenAI 계정이 필요한가요?", a: "아니요. 키, 잔액, 결제는 apiToken.sale에서 관리하며 클라이언트에는 custom base URL과 Bearer 키만 필요합니다." },
      { q: "키 하나로 GPT와 Claude를 모두 쓸 수 있나요?", a: "네. 같은 sk-pool 키와 잔액이 모든 지원 provider를 포함하며 endpoint와 인증 header만 바뀝니다." },
      { q: "OpenAI Platform과 같은 서비스인가요?", a: "아니요. 자체 계정, 선불 잔액, 지원 모델 카탈로그를 가진 독립 OpenAI 호환 gateway입니다." },
    ],
  };
