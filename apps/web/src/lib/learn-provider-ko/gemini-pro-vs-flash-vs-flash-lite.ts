import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Gemini Pro vs Flash vs Flash-Lite",
    h1: "Gemini Pro, Flash, Flash-Lite 비교",
    description: "Gemini Pro, Flash, Flash-Lite를 가격, context, reasoning, 용도별로 비교하고 coding, agent, 대량 API에 맞는 모델을 선택하세요.",
    keywords: ["gemini pro vs flash", "gemini flash vs flash lite", "최고의 gemini 모델", "gemini 모델 비교", "코딩 gemini 모델", "gemini 3.6 flash"],
    dek: "tier를 routing 결정으로 사용하세요. 가장 어려운 reasoning은 Pro, coding 기본은 Flash, 저렴한 대량 단계는 Flash-Lite가 맡습니다. 하나의 키로 모두 사용할 수 있습니다.",
    sections: [
      { h2: "작업별 선택", blocks: [
        { type: "table", headers: ["Tier", "적합한 작업", "권장 현재 ID"], rows: [
          ["Pro", "어려운 reasoning, planning, 깊은 codebase·document 분석", "gemini-3.1-pro-preview"],
          ["Flash", "일상 coding, multimodal agent, 균형 잡힌 production", "gemini-3.6-flash"],
          ["Flash-Lite", "분류, 추출, routing, 저렴한 pre-processing", "gemini-3.1-flash-lite"],
          ["Image", "이미지 생성과 편집", "gemini-3.1-flash-image"],
        ] },
        { type: "p", text: "Gemini 3.6 Flash가 대부분의 새 text workload에 좋은 시작점입니다. 가장 어려운 요청만 Pro로 올리고 예측 가능한 대량 작업은 Flash-Lite로 내립니다." },
      ] },
      { h2: "Context와 비용 trade-off", blocks: [
        { type: "list", items: [
          "현재 text 모델은 1M context와 최대 64K output을 제공합니다.",
          "Pro는 input 200K 이후 long-context premium이 있고 Flash와 Flash-Lite는 창 전체에서 flat rate입니다.",
          "text 모델 cached input은 일반적으로 fresh input의 10%입니다.",
          "큰 요청 전 countTokens를 사용하고 모델 이름보다 실제 eval로 routing하세요.",
        ] },
      ] },
    ],
    faq: [
      { q: "coding에는 어떤 Gemini가 좋은가요?", a: "Gemini 3.6 Flash로 시작하세요. 어려운 architecture와 review는 3.1 Pro Preview, 저렴한 결정적 단계는 Flash-Lite가 적합합니다." },
      { q: "Flash-Lite context가 더 작은가요?", a: "아니요. 게시된 text Flash-Lite도 1M context를 유지하며 단순 작업에서 비용과 latency가 장점입니다." },
      { q: "tier 변경에 새 키가 필요한가요?", a: "아니요. 같은 Gemini base URL과 x-goog-api-key를 유지하고 model ID만 바꾸면 됩니다." },
    ],
  };
