import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "Claude Code에서 Kimi K3 사용",
    h1: "Claude Code에서 Kimi K3와 Kimi for Coding 실행",
    description: "apiToken.sale를 통해 Claude Code에 Kimi K3 또는 Kimi for Coding을 설정하세요. 모든 model tier를 pin하고 1M context를 유지하며 endpoint를 검증합니다.",
    keywords: ["kimi claude code", "kimi k3 claude code", "kimi for coding claude code", "claude code custom model", "claude code kimi api", "k3 1m claude code"],
    dek: "Claude Code는 이미 Anthropic Messages를 사용하므로 Kimi를 직접 실행할 수 있습니다. 안정적인 설정은 모든 내부 model tier를 하나의 Kimi alias로 pin합니다. 그렇지 않으면 main session은 동작해도 subagent가 상속한 Claude model에서 실패할 수 있습니다.",
    sections: [
      { h2: "연결과 모든 model tier pin", blocks: [
        sourceBlock("kimi-api-for-claude-code", 0, 0),
        { type: "p", text: "Anthropic route에서는 bare subscription alias를 사용합니다. k3-256k 또는 kimi-for-coding 같은 256K 모델에는 tier pin을 유지하되 두 개의 1M context 변수는 생략합니다." },
      ] },
      { h2: "모델 소개가 아니라 route 검증", blocks: [
        { type: "list", items: [
          "/status를 열어 Anthropic base URL이 apiToken.sale인지 확인합니다.",
          "모델에게 정체를 묻지 마세요. Claude Code system prompt 때문에 어떤 backend도 Claude라고 답할 수 있습니다.",
          "none/off는 다른 model 선택이 아니라 K3 reasoning 비활성화로 취급하세요. live coverage에서도 K3 tariff가 유지됐고 kimi-k2.6은 public addressable model이 아닙니다.",
          "alias를 장기 pin하기 전에 GET /v1/models를 확인합니다.",
        ] },
      ] },
    ],
    faq: [
      { q: "Claude Code가 Kimi K3를 지원하나요?", a: "네. Claude Code를 https://router.apitoken.sale로 연결하고 모든 model tier를 허용된 Kimi subscription alias에 pin하세요." },
      { q: "왜 모든 Claude Code model variable을 pin해야 하나요?", a: "Claude Code는 main session, tier, subagent 모델을 따로 선택합니다. pin되지 않은 tier는 Claude ID를 상속해 해당 background path가 실행될 때만 실패할 수 있습니다." },
      { q: "Claude Code에서 K3 full 1M context를 유지하려면?", a: "k3 또는 k3[1m]을 사용하고 CLAUDE_CODE_MAX_CONTEXT_TOKENS와 CLAUDE_CODE_AUTO_COMPACT_WINDOW를 모두 1048576으로 설정하세요." },
    ],
  };
