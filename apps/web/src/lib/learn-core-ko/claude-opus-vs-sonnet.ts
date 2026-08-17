import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude Opus 대 Sonnet — 무엇을 쓸까",
    h1: "Claude Opus 대 Sonnet: 어떤 모델을 쓸까",
    description: "Opus인가 Sonnet인가? 코딩과 에이전트에 알맞은 Claude 모델을 고르는 실용 가이드 — 그리고 하나의 apiToken.sale 키·잔액으로 둘 다 사용하기.",
    keywords: ["claude opus vs sonnet", "어떤 claude 모델", "opus 아니면 sonnet 코딩", "최고의 claude 모델", "claude 모델 비교"],
    dek: "Opus와 Sonnet은 서로 다른 문제를 해결합니다. 잘 고르는 것이 더 좋은 결과를 얻고 토큰을 덜 쓰는 가장 쉬운 방법이며, 둘 다 하나의 키로 유지할 수 있습니다.",
    sections: [
      { h2: "기본값은 Sonnet으로", blocks: [
        { type: "p", text: "Sonnet 5와 Sonnet 4.6은 대다수의 코딩과 에이전트 작업을 빠르고 비용 효율적으로 처리합니다. 여기서 시작하세요." },
      ] },
      { h2: "어려운 문제는 Opus로 승격", blocks: [
        { type: "p", text: "복잡한 리팩터, 아키텍처, 추가 추론이 값을 하는 위험도 높은 긴 세션에는 Opus 4.8에 손을 뻗으세요." },
        { type: "note", text: "하나의 키가 둘 다 포괄하므로 공급자를 저글링하지 않고 작업마다 알맞은 등급으로 라우팅할 수 있습니다." },
        { type: "table", headers: ["", "Claude Opus 4.8", "Claude Sonnet 5"], rows: [
          ["공식 가격(입력 / 출력 / 1M)", "$5 / $25", "$3 / $15"],
          ["여기서는 (−50%)", "$2.50 / $12.50", "$1.50 / $7.50"],
          ["컨텍스트 윈도", "1M 토큰", "1M 토큰"],
          ["최적 용도", "어려운 추론, 긴 에이전트 실행", "일상 코딩과 에이전트"],
        ] },
        { type: "link", text: "모든 Claude 모델과 가격 비교", href: "/models" },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
    ],
    faq: [
      { q: "코딩에는 어느 쪽이 더 나은가요?", a: "일상 코딩에는 Sonnet이 권장 기본값이고, 복잡한 추론과 긴 리팩터에는 Opus를 사용하세요." },
      { q: "한 계정에서 둘 다 쓸 수 있나요?", a: "네. Opus, Sonnet, Haiku 모두 동일한 키와 선불 잔액을 공유합니다." },
    ],
  };
