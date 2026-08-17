import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude Haiku API 이용",
    h1: "API로 사용하는 Claude Haiku 4.5",
    description: "apitoken.sale로 Claude Haiku 4.5를 이용하세요. 가장 빠르고 가장 경제적인 Claude 모델로, 대량·저지연 작업에 이상적이며 선불 할인가로 제공됩니다.",
    keywords: ["claude haiku api", "claude haiku 4.5 api", "가장 빠른 claude 모델", "저렴한 claude 모델", "haiku api 키"],
    dek: "Haiku는 속도와 대량 처리를 위해 만들어졌습니다. 분류, 추출, 라우팅, 그리고 깊은 추론보다 지연과 비용이 더 중요한 모든 작업에 적합합니다.",
    sections: [
      { h2: "Haiku가 정답일 때", blocks: [
        { type: "list", items: [
          "대량·저지연 요청.",
          "값싼 백그라운드 작업과 전처리.",
          "Opus가 필요 없는 작업에서 잔액을 오래 쓰기.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "하나의 키로 모델 섞어 쓰기", blocks: [
        { type: "p", text: "모든 모델이 하나의 키와 잔액을 공유하므로, 값싼 작업은 Haiku(claude-haiku-4-5)로 보내고 어려운 요청만 Sonnet이나 Opus로 승격하면 됩니다." },
        { type: "table", headers: ["모델", "공식 입력 / 출력($ / 1M)", "여기서는 (−50%)"], rows: [
          ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
        ] },
        { type: "link", text: "Claude Haiku 4.5 상세 가격(캐시, 컨텍스트, FAQ)", href: "/models/claude-haiku-4-5" },
      ] },
    ],
    faq: [
      { q: "Haiku는 얼마나 빠르고 저렴한가요?", a: "Haiku 4.5는 가장 빠르고 가장 저렴한 Claude 모델로, 대량·지연 민감 작업에 이상적입니다." },
      { q: "Haiku를 다른 모델과 함께 쓸 수 있나요?", a: "네. 하나의 키와 잔액이 Haiku, Sonnet, Opus를 모두 포괄하므로 작업마다 가장 가성비 좋은 모델로 라우팅할 수 있습니다." },
    ],
  };
