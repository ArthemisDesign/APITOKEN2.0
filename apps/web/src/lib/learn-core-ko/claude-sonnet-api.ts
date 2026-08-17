import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude Sonnet API 이용",
    h1: "API로 사용하는 Claude Sonnet",
    description: "apitoken.sale로 Claude Sonnet 5와 Sonnet 4.6을 사용하세요. 일상 코딩과 에이전트의 기본 모델을 공식 API 가격 대비 50% 통일 할인가로 이용할 수 있습니다.",
    keywords: ["claude sonnet api", "claude sonnet 5 api", "sonnet api 키", "claude sonnet 가격", "코딩에 좋은 claude 모델"],
    dek: "Sonnet은 일꾼 모델입니다. 대화형 코딩에 충분히 빠르고, 실제 에이전트 워크플로에 충분히 똑똑합니다. apitoken.sale은 하나의 할인 잔액으로 Sonnet 5와 Sonnet 4.6을 제공합니다.",
    sections: [
      { h2: "일상용 기본 모델", blocks: [
        { type: "p", text: "대부분의 코딩과 에이전트 작업에서 Sonnet이 알맞은 기본값입니다. 품질, 속도, 비용의 균형이 뛰어납니다. 진짜 어려운 문제에는 Opus를 아껴 두세요." },
      ] },
      { h2: "Sonnet 가격 참고", blocks: [
        { type: "p", text: "Claude Sonnet 5(claude-sonnet-5)는 도입 공식 요율로 제공되며, 엔진은 항상 현재 유효 요율을 적용한 뒤 할인을 반영합니다. Sonnet 4.6도 동일한 키로 계속 사용할 수 있습니다." },
        { type: "table", headers: ["모델", "공식 입력 / 출력($ / 1M)", "여기서는 (−50%)"], rows: [
          ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Sonnet 4.6", "$3 / $15", "$1.50 / $7.50"],
        ] },
        { type: "link", text: "Claude Sonnet 5 상세 가격(캐시, 컨텍스트, FAQ)", href: "/models/claude-sonnet-5" },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
    ],
    faq: [
      { q: "어떤 Sonnet 모델을 쓸 수 있나요?", a: "Claude Sonnet 5(claude-sonnet-5)와 Claude Sonnet 4.6을 Opus, Haiku와 동일한 잔액으로 사용할 수 있습니다." },
      { q: "Sonnet은 코딩에 좋나요?", a: "네 — Sonnet은 일상 코딩과 에이전트 워크플로에 권장되는 기본 모델입니다." },
    ],
  };
