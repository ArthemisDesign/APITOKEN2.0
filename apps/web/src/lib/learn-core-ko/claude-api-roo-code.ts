import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Roo Code에서 Claude API 사용하기",
    h1: "Roo Code에서 Claude API 사용하기",
    description: "apitoken.sale로 VS Code의 Roo Code를 Claude에 연결하세요. Anthropic 제공자를 선택하고 커스텀 base URL을 켠 뒤 키를 붙여넣고 50% 통일 할인가로 코딩하세요.",
    keywords: ["claude api roo code", "roo code anthropic", "roo code claude", "roo code 커스텀 base url", "roo code api 키"],
    dek: "Roo Code는 네이티브 Anthropic 제공자와 커스텀 base URL 옵션을 갖춘 에이전트형 VS Code 확장입니다. 할인 게이트웨이 설정은 2분이면 끝납니다.",
    sections: [
      { h2: "세 단계 설정", blocks: [
        { type: "steps", items: [
          "Roo Code 설정을 열고 API 제공자로 Anthropic을 선택하세요.",
          "커스텀 base URL 옵션을 켜고 https://router.apitoken.sale로 설정한 뒤 sk-pool-… 키를 붙여넣으세요.",
          "claude-opus-4-8이나 claude-sonnet-5 같은 모델을 골라 작업을 시작하세요.",
        ] },
        { type: "note", text: "Google 또는 GitHub 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작합니다. 충전 전에 도구를 연결하고 실제 호출을 실행해 보기에 충분한 금액입니다." },
      ] },
      { h2: "Roo Code가 토큰을 태우는 이유 — 그리고 덜 내는 법", blocks: [
        { type: "p", text: "에이전트형 확장은 파일을 읽고, 계획하고, 수정하고, 재검토하는 루프를 돌기 때문에 작업 하나가 많은 모델 호출을 실행할 수 있습니다. 토큰당 할인이 가장 중요한 워크로드가 바로 이것입니다. 같은 세션이 50% 저렴하고, 대시보드에서 토큰 단위로 확인됩니다." },
        { type: "list", items: [
          "일상 작업은 claude-sonnet-5로, 어려운 작업은 claude-opus-4-8로 보내세요.",
          "프롬프트 캐싱은 더 저렴한 공식 캐시 요율로 과금되고 할인이 더해집니다.",
          "키 하나로 Roo Code, Cline, Cursor, SDK를 동시에 커버합니다.",
        ] },
      ] },
    ],
    faq: [
      { q: "Roo Code가 커스텀 Anthropic base URL을 지원하나요?", a: "네 — Anthropic 제공자 설정에 커스텀 base URL 옵션이 있습니다. https://router.apitoken.sale로 설정하고 apitoken.sale 키를 사용하세요." },
      { q: "이 키로 Roo Code에서 어떤 모델을 쓸 수 있나요?", a: "지원되는 모든 Claude 모델 — Opus 4.8과 4.7, Sonnet 5와 4.6, Haiku 4.5 — 을 하나의 키와 선불 잔액으로 사용할 수 있습니다." },
      { q: "Cline과는 무엇이 다른가요?", a: "설정은 거의 동일합니다. 둘 다 커스텀 base URL을 받는 Anthropic 제공자를 갖춘 VS Code 에이전트입니다. 선호하는 쪽을 쓰면 되고, 키는 양쪽에서 모두 작동합니다." },
    ],
  };
