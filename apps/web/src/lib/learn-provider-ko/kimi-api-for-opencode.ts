import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "OpenCode에서 Kimi API 사용",
    h1: "OpenCode에서 Kimi K3와 Kimi for Coding 실행",
    description: "router plugin, key-scoped model catalog, 명시적 kimi/* ID, 하나의 선불 API 키로 OpenCode를 Kimi에 연결하세요.",
    keywords: ["kimi opencode", "kimi api opencode", "kimi k3 opencode", "kimi for coding 설정", "opencode custom provider", "kimi coding agent"],
    dek: "OpenCode는 Kimi namespace를 명시적으로 지정하고 router의 live catalog를 사용합니다. 수동으로 provider limit을 유지하지 않고 K3와 Kimi for Coding을 전환하기에 안전한 coding-agent 설정입니다.",
    sections: [
      { h2: "설치와 검증", blocks: [
        { type: "steps", items: [
          "apiToken.sale OpenCode installer를 실행합니다. router plugin을 기존 config에 merge하고 backup을 남깁니다.",
          "OpenCode를 재시작해 plugin이 key-scoped model catalog를 받게 합니다.",
          "명시적 namespaced model로 결정적 prompt 하나를 실행합니다.",
        ] },
        sourceBlock("kimi-api-for-opencode", 0, 1),
      ] },
      { h2: "Kimi 모델 안전하게 선택", blocks: [
        { type: "list", items: [
          "apitoken/kimi/kimi-for-coding — 경제적인 coding 기본.",
          "apitoken/kimi/kimi-for-coding-highspeed — 두 배 token rate로 더 낮은 latency.",
          "apitoken/kimi/k3-256k — 더 작은 context mode의 K3 reasoning.",
          "apitoken/kimi/k3 — catalog가 노출할 때 full 1M K3.",
        ] },
        { type: "note", text: "Claude Code와 Kimi Code도 Kimi를 지원하지만 설정이 다릅니다. Claude Code는 모든 model tier를 pin해야 하고 Kimi Code는 명시적 OpenAI 호환 provider block을 사용합니다." },
      ] },
    ],
    faq: [
      { q: "OpenCode가 Kimi를 지원하나요?", a: "네. apiToken.sale router plugin이 live Kimi namespace를 등록하고 모델을 apitoken/kimi/{model}로 선택합니다." },
      { q: "static model list보다 plugin이 좋은 이유는?", a: "ID, limit, availability를 key-scoped live catalog와 맞춰 retired 또는 unavailable alias가 local config에 남지 않습니다." },
      { q: "Claude Code도 Kimi를 사용할 수 있나요?", a: "네. 다른 설정으로 가능합니다. Claude Code를 Anthropic endpoint에 연결하고 main, Opus, Sonnet, Haiku, subagent model variables를 하나의 Kimi alias로 pin하세요." },
    ],
  };
