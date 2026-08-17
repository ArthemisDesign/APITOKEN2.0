import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude를 위한 apitoken.sale 대 ProxyAPI",
    h1: "apitoken.sale 대 ProxyAPI",
    description: "Claude API 리셀러 비교: apitoken.sale은 50% 통일 할인, 카드 또는 암호화폐 결제, 모든 모델용 하나의 키를 갖춘 네이티브 Anthropic 엔드포인트를 제공합니다.",
    keywords: ["proxyapi 대안", "apitoken 대 proxyapi", "claude api 리셀러", "proxyapi claude", "proxyapi 없이 claude api"],
    dek: "둘 다 Anthropic 계정 없이 Claude에 접근할 수 있게 해 줍니다. 차이는 결제 방식, 절감 폭, 그리고 엔드포인트가 진정으로 Anthropic 네이티브인지에 있습니다.",
    sections: [
      { h2: "네이티브 Anthropic 엔드포인트", blocks: [
        { type: "p", text: "apitoken.sale은 표준 Anthropic Messages API를 https://router.apitoken.sale에서 노출하므로, Claude Code, Cursor, Anthropic SDK가 그대로 작동합니다. 여러분과 Claude 사이에 어댑터 계층이 없습니다." },
      ] },
      { h2: "마크업이 아닌 할인", blocks: [
        { type: "list", items: [
          "공식 Claude 소비 대비 50% 통일 B2C 할인.",
          "Opus, Sonnet, Haiku를 위한 하나의 선불 키와 잔액.",
          "만료되지 않는 카드 또는 암호화폐 충전.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "각각 언제 맞을까", blocks: [
        { type: "list", items: [
          "apitoken.sale — 통일 할인, 키의 평생 누적 지출 한도, 선택 가능한 만료일을 갖춘 네이티브 Anthropic 엔드포인트.",
          "범용 리셀러 — 이미 그곳의 다른 제공자를 쓰고 있다면 맞을 수 있습니다.",
          "둘 다 Anthropic 계정 장벽을 없애며, 차이는 가격과 Claude 접근이 얼마나 네이티브인가입니다.",
        ] },
      ] },
    ],
    faq: [
      { q: "apitoken.sale이 일반 리셀러보다 저렴한가요?", a: "정가에 마크업을 더하는 대신, 공식 Claude 소비에 50% 통일 할인을 적용합니다." },
      { q: "제 Anthropic 도구가 그대로 작동하나요?", a: "네. 네이티브 Anthropic Messages API이므로 Claude Code, Cursor, SDK는 base URL만 바꾸면 됩니다." },
    ],
  };
