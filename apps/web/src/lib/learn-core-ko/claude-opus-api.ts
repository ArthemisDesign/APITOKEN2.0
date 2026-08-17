import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude Opus API 이용",
    h1: "API로 사용하는 Claude Opus 4.8",
    description: "하나의 apitoken.sale 키로 Claude Opus 4.8 및 4.7을 공식 요율 대비 50% 통일 할인가로 이용하세요. 복잡한 추론, 리팩터, 긴 에이전트 세션에 최적입니다.",
    keywords: ["claude opus api", "claude opus 4.8 api", "opus api 키", "claude opus 가격", "claude opus 할인"],
    dek: "Opus는 Claude의 가장 강력한 등급으로, 어려운 추론·아키텍처·긴 에이전트 실행에 손을 뻗을 모델입니다. apitoken.sale은 다른 모든 모델과 동일한 키·잔액으로 Opus 4.8과 4.7을 제공합니다.",
    sections: [
      { h2: "Opus를 언제 쓸까", blocks: [
        { type: "list", items: [
          "복잡한 리팩터와 여러 파일에 걸친 변경.",
          "아키텍처, 계획 수립, 위험이 큰 추론.",
          "일관성과 캐시 재사용이 중요한 긴 세션.",
        ] },
      ] },
      { h2: "잔액으로 쓰는 Opus", blocks: [
        { type: "p", text: "Opus 4.8(모델 ID claude-opus-4-8)과 Opus 4.7은 공식 토큰 요율에서 할인을 뺀 금액으로 과금되므로, 정가의 일부만으로 최상위 등급을 사용할 수 있습니다." },
        { type: "table", headers: ["모델", "공식 입력 / 출력($ / 1M)", "여기서는 (−50%)"], rows: [
          ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Opus 4.7", "$5 / $25", "$2.50 / $12.50"],
        ] },
        { type: "link", text: "Claude Opus 4.8 상세 가격(캐시, 컨텍스트, FAQ)", href: "/models/claude-opus-4-8" },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
    ],
    faq: [
      { q: "어떤 Opus 모델을 쓸 수 있나요?", a: "Claude Opus 4.8(claude-opus-4-8)과 Claude Opus 4.7을 Sonnet, Haiku와 동일한 키·선불 잔액으로 사용할 수 있습니다." },
      { q: "Opus는 추가 토큰만큼 가치가 있나요?", a: "복잡한 추론, 리팩터, 긴 에이전트 실행에는 그렇습니다. 빠르고 값싼 작업에는 보통 Haiku나 Sonnet이 더 나은 가성비입니다." },
    ],
  };
