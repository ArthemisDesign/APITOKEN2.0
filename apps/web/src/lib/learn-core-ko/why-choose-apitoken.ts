import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "왜 apiToken.sale을 선택하는가",
    h1: "왜 apiToken.sale을 선택하는가",
    description: "Claude, GPT, Gemini, Kimi에 하나의 apiToken.sale 키를 쓰는 이유: 네이티브 또는 호환 API, B2C 50% 할인, 간편한 결제.",
    keywords: ["왜 apitoken.sale", "멀티 프로바이더 api", "claude api 할인", "gpt api 할인", "gemini api 할인", "kimi api 키"],
    dek: "apiToken.sale은 네 가지 프로바이더 제품군을 하나의 키와 선불 잔액으로 묶으면서, 각 클라이언트가 기대하는 네이티브 또는 호환 프로토콜을 유지합니다.",
    sections: [
      { h2: "요약", blocks: [
        { type: "list", items: [
          "Claude와 Kimi에는 Anthropic Messages, GPT와 Kimi를 포함한 multi-provider client에는 OpenAI 호환 routes, Gemini에는 native generateContent를 제공합니다.",
          "지원되는 모든 프로바이더 모델에 공식 소비 대비 50% 통일 할인이 적용되며, 선불 잔액은 만료되지 않습니다.",
          "Anthropic, OpenAI, Google Cloud, Kimi의 별도 결제 계정 없이 즉시 셀프 서비스로 시작할 수 있습니다.",
          "은행 카드 또는 암호화폐로 결제.",
          "키마다 선택 가능한 평생 누적 지출 한도와 만료일, 그리고 대시보드의 토큰 단위 사용량.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "하나의 잔액으로 쓰는 할인된 API 토큰", blocks: [
        { type: "p", text: "잔액을 한 번 선불로 충전하면 공식 소비에서 B2C 50% 할인을 받고 지원되는 Claude, GPT, Gemini, Kimi에 사용할 수 있습니다. 잔액은 만료되지 않고 별도의 고객 구독도 없습니다." },
      ] },
    ],
    faq: [
      { q: "apiToken.sale은 무엇이 다른가요?", a: "하나의 키와 잔액으로 네 가지 프로바이더 제품군을 이용하고 통일 B2C 할인 50%를 받으면서, 클라이언트에는 알맞은 네이티브 또는 호환 프로토콜을 유지합니다." },
      { q: "모든 프로바이더가 하나의 API 형식으로 변환되나요?", a: "아니요. Claude와 Kimi는 Anthropic Messages를, GPT는 OpenAI 호환 routes를, Gemini는 native Google schema를 유지합니다. OpenAI 형식이 필요한 client는 unified route에서 Kimi도 호출할 수 있습니다." },
      { q: "apiToken.sale은 무엇인가요?", a: "Anthropic, OpenAI, Google Cloud, Kimi의 별도 결제 계정 없이 지원되는 Claude, GPT, Gemini, Kimi에 선불로 접근할 수 있는 독립 멀티 프로바이더 API 게이트웨이입니다." },
    ],
  };
