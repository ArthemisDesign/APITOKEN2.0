import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude를 위한 apiToken.sale 대 Portkey",
    h1: "apiToken.sale 대 Portkey",
    description: "Portkey는 여러분의 공급자 키를 사용해 라우팅과 관찰성을 제공하는 AI 게이트웨이입니다. apiToken.sale은 Claude 키와 잔액 자체를 할인가로 제공합니다. 각각 언제 써야 하는지 알아봅니다.",
    keywords: ["portkey 대안", "apitoken vs portkey", "ai 게이트웨이 claude", "portkey claude api", "claude api 게이트웨이"],
    dek: "이 도구들은 서로 다른 문제를 해결합니다. Portkey는 이미 보유한 공급자 키 앞에 놓이고, apiToken.sale은 Claude 키와 할인 잔액이 나오는 곳입니다.",
    sections: [
      { h2: "서로 다른 역할", blocks: [
        { type: "p", text: "Portkey는 여러분이 가져온 API 키 위에 라우팅, 캐싱, 관찰성을 더합니다. Claude 접근권이나 할인을 팔지는 않으며, 그 뒤에는 여전히 충전된 Anthropic 계정이 필요합니다." },
        { type: "p", text: "apiToken.sale은 키와 잔액의 원천입니다. https://router.apitoken.sale의 네이티브 Anthropic 엔드포인트를 50% 통일 할인으로, Anthropic 계정 없이 제공합니다." },
      ] },
      { h2: "함께 쓸 수도 있음", blocks: [
        { type: "p", text: "Portkey의 관찰성이 마음에 든다면, apiToken.sale 키를 Anthropic 공급자로 지정해 그 밑에서 할인을 받을 수 있습니다." },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
    ],
    faq: [
      { q: "Portkey가 Claude 할인을 주나요?", a: "아니요 — Portkey는 이미 보유한 키 위의 게이트웨이입니다. 할인된 Claude 키와 잔액을 제공하는 것은 apiToken.sale입니다." },
      { q: "둘을 함께 쓸 수 있나요?", a: "네. apiToken.sale 키를 Portkey의 Anthropic 공급자로 사용하면 관찰성을 유지하면서 더 적게 지불할 수 있습니다." },
    ],
  };
