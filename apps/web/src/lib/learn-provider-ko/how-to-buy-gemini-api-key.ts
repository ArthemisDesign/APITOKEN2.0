import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "Gemini API 키 구매 방법",
    h1: "Gemini API 키 구매 방법",
    description: "선불 잔액과 카드·암호화폐 결제로 Gemini API 키를 구매하고 native Gemini endpoint에서 Gemini, GPT, Claude, Kimi를 공식 비용의 50%로 사용하세요.",
    keywords: ["gemini api 키 구매", "gemini api 키", "google gemini api", "선불 gemini api", "gemini api 결제", "저렴한 gemini api"],
    dek: "apiToken.sale 키는 별도 Google Cloud billing 없이 native Gemini API를 제공합니다. 한 번 충전하고 x-goog-api-key로 키를 보내며 모든 지원 provider와 잔액을 공유합니다.",
    sections: [
      { h2: "세 단계로 Gemini 키 받기", blocks: [
        { type: "steps", items: [
          "apiToken.sale 계정을 만들고 대시보드에서 sk-pool 키를 발급합니다.",
          "카드 또는 암호화폐로 정수 달러 금액을 충전합니다. 잔액은 만료되지 않습니다.",
          "Gemini base URL을 https://router.apitoken.sale로 설정하고 x-goog-api-key를 사용한 뒤 GET /v1beta/models에서 모델을 선택합니다.",
        ] },
        sourceBlock("how-to-buy-gemini-api-key", 0, 1),
      ] },
      { h2: "사용 가능한 기능", blocks: [
        { type: "list", items: [
          "native Gemini protocol의 Pro, Flash, Flash-Lite text 모델.",
          "Gemini 3.1 Flash Image(Nano Banana 2) 이미지 생성.",
          "Google 형식의 generateContent, streamGenerateContent, countTokens.",
          "고정 50% B2C 할인과 GPT, Claude, Kimi가 공유하는 키/잔액.",
        ] },
        { type: "note", text: "Google SDK base URL에는 bare host만 입력하세요. SDK가 /v1beta를 추가하므로 중복 prefix는 404를 만듭니다." },
      ] },
    ],
    faq: [
      { q: "Google Cloud project가 필요한가요?", a: "아니요. gateway 계정과 billing은 apiToken.sale가 관리하며 클라이언트에는 custom base URL과 sk-pool 키만 필요합니다." },
      { q: "Gemini 인증 header는 무엇인가요?", a: "x-goog-api-key입니다. native Gemini route에서 Anthropic x-api-key나 OpenAI Authorization: Bearer를 사용하지 마세요." },
      { q: "같은 키로 GPT와 Gemini를 호출할 수 있나요?", a: "네. 키와 잔액은 공유되며 provider별 endpoint, protocol, model ID만 바뀝니다." },
    ],
  };
