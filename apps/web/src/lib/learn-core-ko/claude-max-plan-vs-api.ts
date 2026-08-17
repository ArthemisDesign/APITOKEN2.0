import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude Max 요금제 대 Claude API",
    h1: "Claude Max 구독 대 API",
    description: "Claude 구독을 쓸 때와 Claude API를 쓸 때. apiToken.sale은 월 요금 없이 모든 모델에 대한 종량제 API 접근을 50% 통일 할인가로 제공합니다.",
    keywords: ["claude max 요금제", "claude 구독 대 api", "claude max vs api", "claude api 종량제", "구독 없이 claude"],
    dek: "정액 Claude 구독과 종량제 API 과금은 서로 다른 사용에 맞습니다. 프로그래밍적이고 폭발적인 사용에는 보통 선불 잔액의 API가 더 나은 선택입니다.",
    sections: [
      { h2: "구독 대 토큰 단위", blocks: [
        { type: "p", text: "고정 월정액은 하나의 앱에서 꾸준하고 무거운 대화형 사용에 합리적입니다. 하지만 들쭉날쭉한 사용에는 낭비이며, 자신의 도구를 위한 프로그래밍 가능한 API 키를 주지도 않습니다." },
      ] },
      { h2: "왜 API가 더 나은 경우가 많은가", blocks: [
        { type: "list", items: [
          "실제로 사용한 토큰만큼만 지불 — 월 최저 요금 없음.",
          "하나의 키로 Claude Code, Cursor, 에이전트, 프로덕션 호출을 구동.",
          "apiToken.sale은 공식 토큰 요율에서 50%를 통일 할인.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
    ],
    faq: [
      { q: "API가 Claude 구독보다 저렴한가요?", a: "폭발적이거나 프로그래밍적인 사용에는 종량제 API 과금이 유휴 시간에 대한 정액 월 요금을 피하게 해주며, apiToken.sale이 여기에 추가 할인을 더합니다." },
      { q: "코딩 도구에서 API를 쓸 수 있나요?", a: "네 — API 키는 Claude Code, Cursor, VS Code 에이전트, SDK에서 작동하며, 이는 구독이 제공하지 않는 것입니다." },
    ],
  };
