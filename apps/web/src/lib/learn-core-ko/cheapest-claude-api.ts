import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "가장 저렴한 Claude API — 50% 통일 할인",
    h1: "Claude API를 가장 저렴하게 사용하는 방법",
    description: "Claude API 비용을 50% 절감하세요. apitoken.sale은 완전히 동일한 Anthropic Messages API를 선불 할인가로 판매합니다. 같은 모델, 같은 엔드포인트, 더 낮은 토큰당 단가.",
    keywords: ["claude api 저렴", "claude api 할인", "저렴한 claude api", "claude api 가격", "anthropic api 절약", "claude api 싸게"],
    dek: "Claude API는 토큰 단위로 과금되며, 긴 코딩 세션에서는 그 토큰이 빠르게 쌓입니다. apitoken.sale은 선불 잔액을 풀링하고 통일 할인을 적용해 동일한 API를 50% 저렴하게 제공합니다.",
    sections: [
      { h2: "왜 더 저렴한가", blocks: [
        { type: "p", text: "동일한 Anthropic Messages API에 동일한 요청을 보내고 동일한 응답을 받습니다. 내부에서 달라지는 것은 과금뿐입니다. 각 호출은 공식 요율로 측정된 뒤, 잔액에 반영되기 전에 할인이 차감됩니다." },
        { type: "list", items: [
          "B2C 계정은 공식 소비 대비 50% 통일 할인을 받습니다.",
          "모든 요청에 동일한 요율이 적용됩니다 — 해금할 것이 없습니다.",
          "B2B 물량 가격은 별도로 협의합니다.",
        ] },
      ] },
      { h2: "절감 효과가 가장 큰 곳", blocks: [
        { type: "p", text: "에이전트형 코딩, 긴 멀티턴 세션, 프롬프트 캐시를 많이 쓰는 워크플로가 토큰을 가장 많이 소모하므로 절대 절감액도 가장 큽니다. 작업마다 알맞은 모델을 고르면 효과가 한층 더 커집니다." },
        { type: "note", text: "팁: 빠르고 값싼 작업은 Haiku로 보내고, 어려운 추론에는 Opus만 사용해 잔액을 더 오래 쓰세요." },
      ] },
      { h2: "구독 없음, 락인 없음", blocks: [
        { type: "p", text: "월 요금이 없습니다. 만료되지 않는 선불 잔액을 충전하고 요청이 실행될 때만 소비하므로, 사용하지 않는 날에는 비용이 발생하지 않습니다." },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "Claude API 할인이 적용되는 방식", blocks: [
        { type: "p", text: "마진도 없고 별도의 저가 모델도 없습니다. 완전히 동일한 Claude API에 대한 할인 접근을 얻는 것입니다." },
        { type: "list", items: [
          "각 요청은 공식 Anthropic 토큰 요율로 측정됩니다.",
          "통일 50% 할인이 차감됩니다.",
          "차감 후 금액이 선불 잔액에서 빠져나갑니다.",
        ] },
        { type: "table", headers: ["모델", "공식 입력 / 출력($ / 1M)", "여기서는 (−50%)"], rows: [
          ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Opus 4.7", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Sonnet 4.6", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
        ] },
        { type: "link", text: "모델별 전체 가격(캐시 요율 포함)", href: "/models" },
        { type: "link", text: "무료 계산기로 월 비용을 추정해 보세요", href: "/tools/claude-api-cost-calculator" },
      ] },
    ],
    faq: [
      { q: "정말로 같은 Claude API인가요?", a: "네. 동일한 Anthropic Messages API, 동일한 모델 ID, 동일한 요청·응답 형식입니다. 호출당 가격만 더 낮습니다." },
      { q: "얼마나 절약할 수 있나요?", a: "B2C 가격은 모든 요청에서 공식 API 소비 대비 50% 통일 할인입니다." },
      { q: "숨은 요금이나 구독이 있나요?", a: "없습니다. 잔액은 선불이며 만료되지 않고 실제 API 사용에만 소비됩니다. 월 요금은 없습니다." },
      { q: "Anthropic에서 직접 사는 것보다 저렴한 Claude API가 있나요?", a: "네. apiToken.sale은 동일한 Anthropic API를 공식 소비 대비 50% 통일 할인으로, 구독 없이 판매합니다." },
    ],
  };
