import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude API 키 구매 방법",
    h1: "Claude API 키 구매 방법",
    description: "apitoken.sale에서 몇 분 만에 Claude API 키를 구매하세요. 모든 Claude 모델에 통용되는 하나의 키, 선불 잔액, 카드 또는 암호화폐 결제, Anthropic 계정 불필요.",
    keywords: ["claude api 키 구매", "claude api 구매 방법", "claude api 키", "claude api 발급", "anthropic api 키"],
    dek: "Claude를 사용하기 위해 Anthropic 계정도, 초대장도, 법인 카드도 필요 없습니다. apitoken.sale에서는 선불 잔액을 구매하고 키 하나를 발급받아, 동일한 Anthropic Messages API를 할인가로 호출하면 됩니다.",
    sections: [
      { h2: "세 단계로 키 발급받기", blocks: [
        { type: "steps", items: [
          "무료 계정을 만들고 대시보드를 여세요. 승인이나 대기열이 없습니다.",
          "API 키를 하나 생성하세요(sk-pool-… 형태). 동일한 키가 지원되는 모든 Claude 모델에서 작동합니다.",
          "Anthropic 호환 도구를 https://router.apitoken.sale로 지정하고 x-api-key 헤더와 함께 /v1/messages로 요청을 보내세요.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "결제 방식", blocks: [
        { type: "p", text: "원하는 만큼 달러 단위 정수 금액으로 충전하세요. 고정된 상품 카탈로그는 없습니다. 잔액은 선불이며 만료되지 않고, API 요청이 실행될 때만 차감됩니다." },
        { type: "list", items: [
          "안전한 결제 서비스 제공자를 통해 은행 카드 또는 암호화폐로 결제하세요.",
          "모든 요청은 공식 Anthropic API 소비로 환산된 뒤 현재 적용되는 할인이 적용됩니다.",
          "B2C 계정은 모든 요청에서 공식 소비 대비 50% 통일 할인을 받습니다.",
        ] },
      ] },
      { h2: "키로 할 수 있는 일", blocks: [
        { type: "p", text: "키 하나로 지원되는 전체 Claude 라인업(Opus, Sonnet, Haiku)을 Claude Code, Cursor, Cline, Continue, Zed 및 공식 Anthropic SDK에서 사용할 수 있습니다. 프로토콜은 전혀 바뀌지 않으며, 달라지는 것은 가격뿐입니다." },
      ] },
      { h2: "받을 수 있는 Claude 모델과 도구", blocks: [
        { type: "p", text: "Claude API 키 하나로 지원되는 전체 라인업을 하나의 잔액에서 사용할 수 있으며, 모든 Anthropic 호환 도구에서 작동합니다." },
        { type: "list", items: [
          "모델: Claude Opus 4.8 및 4.7, Sonnet 5 및 4.6, Haiku 4.5.",
          "도구: Claude Code, Cursor, Cline, Continue, Zed 및 Anthropic SDK.",
          "형식: 스트리밍과 도구 호출을 지원하는 Anthropic Messages API.",
        ] },
      ] },
    ],
    faq: [
      { q: "Claude API 키를 사려면 Anthropic 계정이 필요한가요?", a: "아니요. apitoken.sale이 자체 키와 잔액을 발급하므로 Anthropic 계정, 초대장, 승인 없이 시작할 수 있습니다." },
      { q: "키는 얼마나 빨리 활성화되나요?", a: "즉시 활성화됩니다. 대시보드에서 키를 생성하면 다음 요청부터 바로 작동하며, 대기열이나 수동 심사가 없습니다." },
      { q: "시작하는 데 비용이 얼마나 드나요?", a: "달러 단위 정수 금액으로 원하는 만큼 충전할 수 있으며, Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧도 받습니다." },
      { q: "이것이 공식 Claude API인가요?", a: "네 — 동일한 Anthropic Messages API와 동일한 Claude 모델을 제공합니다. 다른 것은 가격과 가입·결제 방식뿐입니다." },
    ],
  };
