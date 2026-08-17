import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "코딩에 가장 좋은 Claude 모델",
    h1: "코딩에 가장 좋은 Claude 모델",
    description: "코딩에 어떤 Claude 모델을 써야 할까요? 작업별로 Opus, Sonnet, Haiku를 고르는 실용 가이드 — 모두 하나의 apiToken.sale 키로 이용 가능합니다.",
    keywords: ["코딩에 좋은 claude 모델", "프로그래밍용 claude 모델", "opus vs sonnet 코딩", "claude 코딩 모델", "코드에 어떤 claude"],
    dek: "가장 좋은 모델은 작업에 따라 다릅니다. 모델을 작업에 맞추면 더 적은 토큰으로 더 나은 결과를 얻으며, 모든 등급이 하나의 키에 있습니다.",
    sections: [
      { h2: "일상 코딩에는 Sonnet", blocks: [
        { type: "p", text: "Claude Sonnet 5와 Sonnet 4.6은 대화형 코딩과 에이전트 루프의 기본값입니다. 빠르고, 유능하며, 비용 효율적입니다. 대부분의 작업은 여기서 시작하세요." },
      ] },
      { h2: "어려운 문제에는 Opus", blocks: [
        { type: "p", text: "복잡한 리팩터, 아키텍처, 추가 추론이 값을 하는 위험도 높은 긴 세션에는 Claude Opus 4.8을 사용하세요." },
      ] },
      { h2: "대량 처리에는 Haiku", blocks: [
        { type: "p", text: "Claude Haiku 4.5는 린팅, 추출, 빠른 편집 같은 빠르고 값싼 대량 작업을 처리해 잔액을 오래 쓰게 해줍니다." },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
    ],
    faq: [
      { q: "코딩에 가장 좋은 Claude 모델은?", a: "일상 코딩에는 Sonnet, 복잡한 추론과 리팩터에는 Opus, 빠른 대량 작업에는 Haiku입니다. 모두 하나의 apiToken.sale 키로 사용합니다." },
      { q: "요청마다 모델을 바꿀 수 있나요?", a: "네. 하나의 키와 잔액이 모든 모델을 포괄하므로 요청마다 가장 가성비 좋은 등급으로 라우팅할 수 있습니다." },
    ],
  };
