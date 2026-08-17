import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "GPT-5.6 Sol vs Terra vs Luna",
    h1: "GPT-5.6 Sol, Terra, Luna 비교",
    description: "GPT-5.6 Sol, Terra, Luna를 가격, reasoning effort, context, 용도별로 비교하고 coding과 production에 맞는 GPT 모델을 선택하세요.",
    keywords: ["gpt-5.6 sol vs terra", "gpt-5.6 terra vs luna", "최고의 gpt-5.6 모델", "gpt-5.6 모델", "gpt-5.6 비교", "코딩 gpt 모델"],
    dek: "GPT-5.6 제품군은 400K context, 최대 128K output, 전체 reasoning-effort 범위를 공유합니다. 실질적인 차이는 token당 구매하는 능력과 latency입니다.",
    sections: [
      { h2: "작업별 선택", blocks: [
        { type: "table", headers: ["Tier", "적합한 작업", "공식 input / output"], rows: [
          ["Sol", "어려운 reasoning, 장기 agent, 복잡한 code review", "$5 / $30"],
          ["Terra", "일상 coding, production chat, 균형 잡힌 agent", "$2 / $12"],
          ["Luna", "분류, 추출, routing, 대량 단순 작업", "$0.20 / $1.20"],
        ] },
        { type: "p", text: "Terra가 가장 안전한 기본값입니다. Sol의 controls와 context를 40% 가격에 유지합니다. eval에서 품질 차이가 확인되면 Sol로 올리고 예측 가능한 대량 작업은 Luna로 보냅니다." },
      ] },
      { h2: "공통 기능", blocks: [
        { type: "list", items: [
          "400K context와 최대 128K output.",
          "text와 image input, text output.",
          "Responses와 Chat Completions의 SSE streaming.",
          "GPT-5.6 line에서 none부터 max까지 reasoning effort.",
          "동일한 endpoint, 키, 잔액으로 작업별 모델 전환.",
        ] },
      ] },
    ],
    faq: [
      { q: "coding에 가장 좋은 GPT-5.6은 무엇인가요?", a: "일상 coding은 Terra로 시작하세요. 가장 어려운 architecture와 agent에는 Sol, 저렴한 결정적 sub-step에는 Luna가 적합합니다." },
      { q: "Sol, Terra, Luna에 서로 다른 endpoint가 필요한가요?", a: "아니요. 세 모델 모두 같은 OpenAI 호환 base URL과 키를 사용하며 model ID만 바뀝니다." },
      { q: "Terra가 max reasoning effort를 지원하나요?", a: "네. Sol, Terra, Luna 모두 max를 포함한 같은 GPT-5.6 reasoning 범위를 제공합니다." },
    ],
  };
