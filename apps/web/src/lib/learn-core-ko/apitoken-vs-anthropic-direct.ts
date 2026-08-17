import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "apiToken.sale 대 Anthropic 직접 구매",
    h1: "apiToken.sale 대 Anthropic에서 직접 구매",
    description: "apiToken.sale과 Anthropic 직접 구매 비교: 동일한 Messages API와 모델이지만 50% 통일 할인, 계정 요구 없음, 카드 또는 암호화폐 결제.",
    keywords: ["claude api anthropic 직접 비교", "apitoken vs anthropic", "anthropic api 대안", "anthropic api보다 저렴", "claude api 리셀러"],
    dek: "apiToken.sale은 다른 API가 아닙니다. 선불 잔액에서 할인가로 재판매되는 동일한 Anthropic Messages API입니다. 실제로 무엇이 바뀌고 무엇이 그대로인지 살펴보겠습니다.",
    sections: [
      { h2: "그대로 유지되는 것", blocks: [
        { type: "list", items: [
          "동일한 Anthropic Messages API, 엔드포인트, 스트리밍.",
          "동일한 모델 ID(claude-opus-4-8, claude-sonnet-5, claude-haiku-4-5).",
          "여러분 코드가 이미 기대하는 동일한 요청·응답 형식.",
        ] },
      ] },
      { h2: "바뀌는 것", blocks: [
        { type: "list", items: [
          "가격: B2C의 경우 공식 소비 대비 50% 통일 할인.",
          "온보딩: Anthropic 계정, 대기열, 청구 국가 요구 없음.",
          "결제: 은행 카드 또는 암호화폐.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "각각 누구에게 맞는가", blocks: [
        { type: "p", text: "이미 마찰 없는 Anthropic 청구와 엔터프라이즈 계약이 있다면 직접 구매가 맞을 수 있습니다. 같은 모델을 더 싸게, 더 빠르게 시작하고, 카드나 암호화폐로 결제하고 싶다면 apiToken.sale이 실용적인 선택입니다." },
      ] },
    ],
    faq: [
      { q: "apiToken.sale이 진짜 Claude API인가요?", a: "네 — 동일한 Anthropic Messages API와 모델을 제공합니다. 가격과 온보딩만 다릅니다." },
      { q: "왜 Anthropic 직접 구매보다 저렴한가요?", a: "잔액이 선불로 풀링되고, 공식 소비에 50%의 통일 할인이 적용되기 때문입니다." },
    ],
  };
