import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Kimi K3 vs Kimi for Coding",
    h1: "Kimi K3와 Kimi for Coding 비교",
    description: "Kimi K3, K3 256K, Kimi for Coding, High Speed를 context, reasoning control, latency, token 가격으로 비교합니다.",
    keywords: ["kimi k3 vs kimi for coding", "kimi k3 api", "kimi k2.7 code", "최고의 kimi 코딩 모델", "kimi 모델 비교", "kimi highspeed"],
    dek: "K3는 reasoning과 long context 제품군이고 Kimi for Coding은 경제적인 coding 제품군입니다. High Speed는 두 배 rate로 latency를 낮추며 K3 alias는 256K 또는 1M mode를 선택합니다.",
    sections: [
      { h2: "모델 제품군 맵", blocks: [
        { type: "table", headers: ["공개 ID", "Context", "적합한 작업"], rows: [
          ["kimi/kimi-for-coding", "256K", "일상 coding과 경제적 agent loop"],
          ["kimi/kimi-for-coding-highspeed", "256K", "속도가 비용을 정당화하는 latency-sensitive coding"],
          ["kimi/k3-256k", "256K", "full context가 필요 없는 K3 reasoning"],
          ["kimi/k3 · kimi/k3[1m]", "1M", "대형 codebase, document, 어려운 reasoning"],
        ] },
        { type: "p", text: "k3[1m]은 K3 1M mode의 compatibility spelling이며 별도 모델이 아닙니다. router가 provider의 실제 k3 wire model로 normalize합니다." },
      ] },
      { h2: "Reasoning과 routing", blocks: [
        { type: "list", items: [
          "K3는 low, high, max reasoning effort를 지원하며 기본은 high입니다.",
          "Kimi for Coding과 High Speed는 thinking이 켜져 있습니다.",
          "alias를 고정하기 전 key-scoped /v1/models를 확인합니다.",
          "실용적 router는 일상 code를 Kimi for Coding으로, 크고 어려운 작업을 K3로 보냅니다.",
        ] },
      ] },
    ],
    faq: [
      { q: "coding에 가장 좋은 Kimi 모델은?", a: "Kimi for Coding이 경제적인 기본입니다. 어려운 reasoning이나 long context에는 K3, 두 배 가격보다 낮은 latency가 중요할 때만 High Speed를 사용하세요." },
      { q: "k3와 k3[1m]은 다른 모델인가요?", a: "아니요. 같은 K3 1M mode를 선택하며 bracket 형식은 compatibility alias입니다." },
      { q: "내부 official model ID를 요청할 수 있나요?", a: "아니요. kimi-k2.7-code 같은 tariff ID가 아니라 router catalog의 공개 subscription alias를 사용하세요." },
    ],
  };
