import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "VS Code에서 Claude API 사용하기 (Cline, Continue)",
    h1: "VS Code에서 Claude API 사용하기",
    description: "apitoken.sale 키로 Cline 또는 Continue를 사용해 VS Code에서 Claude를 실행하세요. Anthropic base URL을 router.apitoken.sale로 설정하고 토큰 단위로 할인가에 결제하세요.",
    keywords: ["claude api vs code", "cline claude api", "continue claude api", "vscode claude", "vscode anthropic api 키"],
    dek: "Cline이나 Continue 같은 무료 VS Code 에이전트는 Anthropic 호환 엔드포인트를 모두 지원하므로, VS Code 안에서 할인된 잔액으로 Claude 코딩을 할 수 있습니다.",
    sections: [
      { h2: "Cline", blocks: [
        { type: "code", code: `# Cline → Settings\nAPI Provider : Anthropic\nBase URL     : https://router.apitoken.sale\nAPI Key      : sk-pool-•••\nModel        : claude-opus-4-8` },
      ] },
      { h2: "Continue", blocks: [
        { type: "code", code: `// ~/.continue/config.json\n{\n  "models": [{\n    "title": "Claude via apiToken.sale",\n    "provider": "anthropic",\n    "apiBase": "https://router.apitoken.sale",\n    "apiKey": "sk-pool-•••",\n    "model": "claude-opus-4-8"\n  }]\n}` },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "어떤 확장을 쓸지와 문제 해결", blocks: [
        { type: "p", text: "Cline은 자율 편집에 강한 기본 선택지이고, Continue는 더 가벼워서 인라인 채팅과 자동완성에 좋습니다. 둘 다 무료이며 선불 잔액을 사용합니다." },
        { type: "list", items: [
          "401 Unauthorized: API 키 또는 base URL이 잘못되었습니다.",
          "모델을 찾을 수 없음: claude-sonnet-5 또는 claude-opus-4-8 같은 최신 ID를 사용하세요.",
          "느리거나 429: 동시 요청 수를 줄이고 Retry-After를 준수하세요.",
        ] },
      ] },
    ],
    faq: [
      { q: "어떤 VS Code 확장이 작동하나요?", a: "Cline과 Continue를 포함해 Anthropic 호환 엔드포인트를 지원하는 모든 확장이 apitoken.sale 키와 함께 작동합니다." },
      { q: "유료 확장이 필요한가요?", a: "아니요. Cline과 Continue는 무료이며, 선불 잔액에서 차감되는 Claude API 사용량에 대해서만 비용을 지불합니다." },
    ],
  };
