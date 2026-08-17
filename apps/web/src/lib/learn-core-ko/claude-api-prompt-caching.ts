import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude API의 프롬프트 캐싱",
    h1: "Claude 프롬프트 캐싱으로 비용 줄이기",
    description: "프롬프트 캐싱은 Claude API에서 반복되는 컨텍스트를 훨씬 저렴하게 만듭니다. apiToken.sale에서 작동하는 방식, 언제 쓸지, 할인과 어떻게 겹쳐지는지 알아봅니다.",
    keywords: ["claude 프롬프트 캐싱", "claude api 캐시", "anthropic prompt cache", "캐싱으로 claude 비용 절감", "claude cache read"],
    dek: "시스템 프롬프트, 파일, 도구 정의처럼 같은 큰 컨텍스트를 반복해서 보낸다면, 캐싱이 그 토큰을 비싼 것에서 거의 공짜로 바꿔줍니다.",
    sections: [
      { h2: "캐싱이 비용을 아끼는 원리", blocks: [
        { type: "p", text: "캐시 쓰기와 캐시 읽기는 별도로 측정되며, 캐시 읽기는 새 입력 토큰의 일부 비용입니다. 안정적이고 재사용되는 컨텍스트가 이상적인 대상입니다." },
      ] },
      { h2: "할인과 겹쳐집니다", blocks: [
        { type: "p", text: "캐싱은 토큰 개수를 낮추고, apiToken.sale 할인은 토큰당 가격을 낮춥니다. 둘이 합쳐지면 청구액이 훨씬 작아지며, 모든 캐시 항목이 사용량 내역에 표시됩니다." },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
    ],
    faq: [
      { q: "프롬프트 캐싱은 얼마나 절약되나요?", a: "캐시 읽기는 새 입력 토큰의 일부 비용이므로, 반복되는 큰 컨텍스트가 훨씬 저렴해집니다." },
      { q: "캐싱이 할인과 함께 작동하나요?", a: "네 — 캐싱은 토큰 개수를 줄이고 할인은 토큰당 가격을 줄이므로 절감 효과가 곱해집니다." },
    ],
  };
