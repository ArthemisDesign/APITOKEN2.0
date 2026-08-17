import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude를 위한 apitoken.sale 대 OpenRouter",
    h1: "Claude를 위한 apitoken.sale 대 OpenRouter",
    description: "Claude 게이트웨이를 고르시나요? apitoken.sale과 OpenRouter를 비교하세요. 네이티브 Anthropic 엔드포인트와 선불 할인 대 멀티 제공자 라우터.",
    keywords: ["openrouter 대안", "apitoken 대 openrouter", "claude api 게이트웨이", "openrouter claude", "최고의 claude api 게이트웨이"],
    dek: "둘 다 Anthropic 계정 없이 Claude에 접근할 수 있게 해 주지만, 구조가 다릅니다. Claude가 주력 모델이라면 네이티브 Anthropic 엔드포인트가 일을 단순하게 유지합니다.",
    sections: [
      { h2: "네이티브 Anthropic 엔드포인트", blocks: [
        { type: "p", text: "apitoken.sale은 표준 Anthropic Messages API를 https://router.apitoken.sale에서 노출하므로, Claude Code, Cursor, Anthropic SDK가 어댑터 없이 작동합니다. 범용 멀티 제공자 추상화 계층을 거치지 않습니다." },
      ] },
      { h2: "마크업이 아닌 선불 할인", blocks: [
        { type: "list", items: [
          "공식 Claude 소비 대비 50% 통일 B2C 할인.",
          "Opus, Sonnet, Haiku를 위한 하나의 키와 잔액.",
          "만료되지 않는 카드 또는 암호화폐 충전.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "각각 언제 고를까", blocks: [
        { type: "list", items: [
          "apitoken.sale — Claude가 주력 모델이고 할인이 있는 네이티브 Anthropic 엔드포인트를 원할 때.",
          "OpenRouter — 하나의 추상화 뒤에서 여러 제공자로 라우팅해야 할 때.",
          "둘 다 Anthropic 계정 없이 시작할 수 있지만, Claude 소비를 직접 할인해 주는 것은 apitoken.sale뿐입니다.",
        ] },
      ] },
    ],
    faq: [
      { q: "왜 Claude 네이티브 게이트웨이를 골라야 하나요?", a: "Claude가 주력 모델이라면 네이티브 Anthropic 엔드포인트 덕분에 기존 Anthropic 도구와 SDK가 그대로 작동합니다." },
      { q: "apitoken.sale은 가격에 마크업을 붙이나요?", a: "아니요. 마크업을 더하는 대신 공식 Claude 소비에 할인을 적용합니다." },
    ],
  };
