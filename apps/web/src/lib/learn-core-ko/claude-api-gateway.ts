import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude API 게이트웨이란 무엇인가?",
    h1: "Claude API 게이트웨이란",
    description: "Claude API 게이트웨이는 도구와 Anthropic 사이에 위치해 접근, 과금, 제어를 더합니다. apitoken.sale은 50% 통일 할인을 제공하는 네이티브 게이트웨이입니다.",
    keywords: ["claude api 게이트웨이", "api 게이트웨이란", "anthropic 게이트웨이", "claude 프록시", "claude api 접근 계층"],
    dek: "게이트웨이는 코드와 모델 제공자 사이의 얇은 계층입니다. 좋은 Claude 게이트웨이는 도구에는 보이지 않으면서 접근, 가격, 제어를 개선합니다.",
    sections: [
      { h2: "게이트웨이가 하는 일", blocks: [
        { type: "list", items: [
          "표준 Anthropic Messages API를 제시해 도구가 그대로 작동합니다.",
          "접근과 과금을 처리합니다 — 여기서는 할인된 선불 잔액.",
          "키별 평생 누적 지출 한도, 선택 가능한 만료일, 사용량 가시성을 더합니다.",
        ] },
      ] },
      { h2: "번역 계층이 아닌 네이티브", blocks: [
        { type: "p", text: "apitoken.sale은 Anthropic 네이티브입니다. 어떤 클라이언트든 https://router.apitoken.sale/v1/messages로 지정하면 api.anthropic.com과 완전히 동일하게 동작하며, 여기에 할인과 대시보드 제어가 더해집니다." },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "게이트웨이에서 살펴봐야 할 것", blocks: [
        { type: "list", items: [
          "네이티브 Anthropic API — 도구와 SDK가 그대로 작동.",
          "대시보드에서 감사할 수 있는 투명한 토큰 단위 과금.",
          "키별 제어: 선택 가능한 평생 누적 지출 한도와 만료일.",
          "락인 없음 — 만료되지 않는 선불 잔액.",
        ] },
      ] },
    ],
    faq: [
      { q: "게이트웨이가 API를 바꾸나요?", a: "아니요. 네이티브 Claude 게이트웨이는 표준 Anthropic Messages API를 사용하므로 도구와 SDK가 그대로입니다." },
      { q: "왜 Anthropic을 직접 쓰지 않고 게이트웨이를 쓰나요?", a: "할인, Anthropic 계정 없는 즉시 접근, 개별 키의 선택 가능한 평생 누적 지출 한도와 만료일을 위해서입니다." },
    ],
  };
