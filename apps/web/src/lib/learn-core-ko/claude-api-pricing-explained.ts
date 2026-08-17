import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude API 가격 이해하기",
    h1: "Claude API 가격은 어떻게 작동하는가",
    description: "Claude API 가격 이해하기: 토큰당 입력·출력 요율, 프롬프트 캐싱, 그리고 apiToken.sale이 50% 통일 할인을 적용하는 방식.",
    keywords: ["claude api 가격", "claude api 비용", "claude api 가격 구조", "claude 토큰 가격", "anthropic api 가격 설명"],
    dek: "Claude는 입력과 출력에 각각 토큰 단위로 과금되며, 캐시된 콘텐츠에는 할인이 있습니다. apiToken.sale은 이 원리를 그대로 유지하고 그 위에 할인을 얹습니다.",
    sections: [
      { h2: "토큰, 입력과 출력", blocks: [
        { type: "p", text: "모든 요청은 입력 토큰(프롬프트와 컨텍스트)과 출력 토큰(모델의 응답)으로 측정됩니다. 출력 토큰은 보통 입력보다 비싸고, 큰 모델일수록 토큰당 비용이 높습니다." },
      ] },
      { h2: "캐싱과 사고(thinking)", blocks: [
        { type: "list", items: [
          "캐시 쓰기와 캐시 읽기는 별도로 측정되며, 캐시 읽기가 훨씬 저렴합니다.",
          "추론이 많은 호출에서는 사고 토큰이 출력에 산입됩니다.",
          "스트리밍과 비스트리밍 요청은 동일하게 과금됩니다.",
        ] },
      ] },
      { h2: "apiToken.sale 할인", blocks: [
        { type: "p", text: "각 호출은 공식 Anthropic 소비로 환산된 뒤 할인이 차감됩니다. B2C는 모든 요청에서 50% 통일 할인이 적용됩니다. 모든 요청은 토큰 단위 상세와 함께 대시보드에서 확인할 수 있습니다." },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "모델별 Claude API 토큰 가격", blocks: [
        { type: "p", text: "큰 모델일수록 토큰당 비용이 높습니다. Opus는 프리미엄 등급, Sonnet은 균형 잡힌 기본값, Haiku는 가장 저렴합니다. 할인은 모든 모델에 적용되므로 순위는 그대로지만 모든 가격이 낮아집니다." },
        { type: "table", headers: ["모델", "공식 입력 / 출력($ / 1M)", "여기서는 (−50%)"], rows: [
          ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Opus 4.7", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Sonnet 4.6", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
        ] },
        { type: "link", text: "캐시 요율과 컨텍스트 윈도가 있는 모델 페이지", href: "/models" },
      ] },
    ],
    faq: [
      { q: "Claude API는 어떻게 과금되나요?", a: "토큰 단위로 입력과 출력으로 나뉘며, 캐시 읽기에는 별도의 더 저렴한 요율이 적용됩니다. 큰 모델일수록 토큰당 비용이 높습니다." },
      { q: "할인은 어떻게 적용되나요?", a: "먼저 공식 소비가 계산되고, 잔액에 반영되기 전에 B2C 통일 50% 할인이 차감됩니다." },
      { q: "Claude API 토큰은 어떻게 가격이 매겨지나요?", a: "토큰 단위로 입력과 출력이 나뉘며 캐시 읽기는 더 저렴합니다. apiToken.sale은 공식 토큰 요율 위에 50% 통일 할인을 적용합니다." },
    ],
  };
