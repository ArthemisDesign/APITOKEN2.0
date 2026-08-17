import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "Kimi Code에서 apiToken.sale 사용",
    h1: "Kimi Code에서 Kimi, Claude, GPT, Gemini 실행",
    description: "OpenAI 호환 provider config로 Kimi Code를 apiToken.sale에 연결하고 namespaced model을 선언하며 config.toml의 API 키를 보호하세요.",
    keywords: ["kimi code api", "kimi code custom provider", "kimi code config toml", "kimi code api 키", "kimi code k3", "kimi code openai 호환"],
    dek: "Kimi Code는 custom OpenAI 호환 provider를 받으므로 하나의 apiToken.sale provider entry로 unified catalog에 접근할 수 있습니다. 각 모델은 실제 namespace와 검증된 context window로 별도 선언해야 합니다.",
    sections: [
      { h2: "설치하고 provider 선언", blocks: [
        sourceBlock("kimi-api-for-kimi-code", 0, 0),
        { type: "note", text: "/login을 실행하지 마세요. CLI가 Kimi membership에 연결됩니다. Kimi Code는 custom-provider credential을 config.toml에만 저장하므로 파일에 plain text 키가 들어가며 권한을 제한해야 합니다." },
      ] },
      { h2: "실행, 검증, 모델 추가", blocks: [
        sourceBlock("kimi-api-for-kimi-code", 1, 0),
        { type: "list", items: [
          "/status에 provider base URL이 https://router.apitoken.sale/v1로 표시되어야 합니다.",
          "model field는 kimi/k3, openai/gpt-5.6-terra, google/gemini-3.6-flash 같은 unified catalog namespace를 사용합니다.",
          "추가 모델마다 검증된 max_context_size를 config.toml에 선언하세요. Kimi Code가 이 값으로 context compact 시점을 결정합니다.",
        ] },
      ] },
    ],
    faq: [
      { q: "Kimi Code가 apiToken.sale 키를 사용할 수 있나요?", a: "네. base_url이 https://router.apitoken.sale/v1인 OpenAI 호환 provider를 추가하고 Kimi Code config.toml에 키를 저장하세요." },
      { q: "Kimi Code에서 Kimi 외 모델도 실행할 수 있나요?", a: "네. 같은 provider entry로 unified catalog에 접근하며 각 Claude, GPT, Gemini, Kimi 모델을 namespaced ID와 올바른 context limit으로 선언합니다." },
      { q: "chmod 600이 왜 중요한가요?", a: "Kimi Code는 shell에서 custom-provider credential을 읽지 않습니다. raw API 키가 config.toml에 있으므로 계정 소유자만 읽을 수 있어야 합니다." },
    ],
  };
