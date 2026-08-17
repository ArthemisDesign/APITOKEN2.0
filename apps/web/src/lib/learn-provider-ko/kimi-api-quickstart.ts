import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "Kimi API 빠른 시작",
    h1: "Anthropic SDK로 Kimi API 빠르게 시작하기",
    description: "apiToken.sale에서 Kimi K3와 Kimi for Coding을 호출하세요. Anthropic Messages, x-api-key, namespaced model ID, terminal usage, 공유 잔액을 설명합니다.",
    keywords: ["kimi api 빠른 시작", "kimi api 튜토리얼", "kimi anthropic api", "kimi k3 api 예제", "kimi for coding api", "kimi api curl"],
    dek: "Kimi는 unified router에서 Anthropic Messages protocol을 사용합니다. 기존 Anthropic client에는 custom base URL, apiToken.sale 키, 명시적 kimi/* model ID만 필요합니다.",
    sections: [
      { h2: "curl로 첫 요청", blocks: [
        sourceBlock("kimi-api-quickstart", 0, 0),
        { type: "p", text: "terminal usage가 Anthropic 형식이므로 기존 usage parser를 그대로 사용할 수 있습니다. route는 stream: true를 받지만 provider boundary의 증분성은 아직 live 검증 중입니다." },
      ] },
      { h2: "Anthropic Python SDK 사용", blocks: [
        sourceBlock("kimi-api-quickstart", 1, 0),
        { type: "note", text: "kimi-k2.7-code 같은 Open Platform ID로 바꾸지 마세요. public router는 GET /v1/models의 subscription alias를 받으며 OpenAI 호환 client도 unified /v1 route에서 같은 Kimi alias를 호출합니다." },
      ] },
    ],
    faq: [
      { q: "Anthropic SDK로 Kimi를 호출할 수 있나요?", a: "네. base_url을 https://router.apitoken.sale로 설정하고 key-scoped catalog의 kimi/* model ID를 선택하세요." },
      { q: "Kimi route에 stream: true를 설정할 수 있나요?", a: "route는 이 parameter를 받지만 upstream과 public chunk의 증분성은 아직 live 검증 중입니다. chunk 도착 timing이 중요하면 non-stream mode를 사용하세요." },
      { q: "어떤 model ID로 시작해야 하나요?", a: "coding 기본값은 kimi/kimi-for-coding, full 1M 없이 K3 reasoning이 필요하면 kimi/k3-256k가 적합합니다." },
    ],
  };
