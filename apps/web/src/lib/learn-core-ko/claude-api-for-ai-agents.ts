import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "AI 에이전트를 위한 Claude API",
    h1: "AI 에이전트에 Claude API 사용하기",
    description: "apitoken.sale로 Claude API 위에 AI 에이전트를 구축하세요. 모든 모델용 하나의 키, 스트리밍, 도구 사용, 프롬프트 캐싱, 그리고 긴 실행 비용을 제어하는 키의 평생 누적 지출 한도.",
    keywords: ["claude api 에이전트", "claude ai 에이전트 api", "claude 도구 사용", "claude 에이전트 프레임워크", "claude api 자동화"],
    dek: "에이전트형 워크로드는 토큰을 많이 쓰고 오래 실행되므로 모델 선택, 캐싱, 비용 제어가 가장 중요합니다. apitoken.sale이 에이전트에 어떻게 맞는지 살펴봅니다.",
    sections: [
      { h2: "에이전트에 필요한 것", blocks: [
        { type: "list", items: [
          "스트리밍과 도구 사용 — 둘 다 Anthropic Messages API의 표준.",
          "모델 라우팅: 값싼 단계는 Haiku, 추론은 Sonnet, 가장 어려운 것은 Opus.",
          "반복되는 시스템 프롬프트와 도구 정의를 위한 프롬프트 캐싱.",
          "폭주하는 루프가 키의 한도를 넘겨 지출하지 못하도록 하는 평생 누적 지출 한도.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "비용을 의식한 에이전트 루프", blocks: [
        { type: "p", text: "실전 패턴: 계획과 추론은 Sonnet으로, 값싼 하위 단계와 파싱은 Haiku로 라우팅하고, 가장 어려운 호출만 Opus로 올리세요. 시스템 프롬프트와 도구 정의를 캐시해 반복되는 컨텍스트를 거의 무료로 만드세요." },
        { type: "list", items: [
          "폭주하는 루프가 한도를 넘겨 지출하지 못하도록 키의 평생 누적 지출 한도를 설정하세요.",
          "에이전트가 부분 출력에 반응할 수 있도록 스트리밍하세요.",
          "토큰 사용량을 살펴 어떤 단계가 어떤 모델을 쓸지 조정하세요.",
        ] },
      ] },
    ],
    faq: [
      { q: "Claude API가 에이전트에 좋나요?", a: "네. 스트리밍, 도구 사용, 모델 라우팅, 프롬프트 캐싱을 모두 하나의 apitoken.sale 키와 소비 제어로 제공합니다." },
      { q: "에이전트 비용을 어떻게 낮추나요?", a: "값싼 단계는 Haiku로 라우팅하고 반복되는 컨텍스트를 캐시하며 에이전트 키에 평생 누적 지출 한도를 설정하세요." },
    ],
  };
