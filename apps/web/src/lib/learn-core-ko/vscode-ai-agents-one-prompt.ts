import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude로 무료 VS Code AI 에이전트 실행하기",
    h1: "Claude로 무료 VS Code AI 에이전트 실행하기",
    description: "Cursor Pro 없이 apitoken.sale Claude 키로 Cline, Roo Code 같은 무료 VS Code 에이전트를 설정하세요. 하나의 엔드포인트, 모든 Claude 모델, 할인가.",
    keywords: ["무료 vscode ai 에이전트", "cline roo code claude", "vscode claude 에이전트", "cursor 무료 대안", "cursor 없이 claude vscode"],
    dek: "에이전트형 코딩을 하려고 Cursor Pro가 필요하지는 않습니다. 무료 VS Code 에이전트는 Anthropic 호환 키를 모두 받아들이므로, Claude가 할인된 잔액으로 VS Code에서 실행됩니다.",
    sections: [
      { h2: "에이전트를 Claude로 지정하기", blocks: [
        { type: "steps", items: [
          "Cline이나 Roo Code 같은 무료 에이전트 확장을 설치하세요.",
          "API 제공자로 Anthropic을 선택하세요.",
          "base URL을 https://router.apitoken.sale로 설정하고 sk-pool-••• 키를 붙여넣은 뒤 claude-sonnet-5 같은 모델을 선택하세요.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "작업마다 알맞은 모델 고르기", blocks: [
        { type: "list", items: [
          "claude-sonnet-5 — 일상적인 코딩과 에이전트 루프의 기본값.",
          "claude-opus-4-8 — 복잡한 리팩터링, 아키텍처, 긴 세션.",
          "claude-haiku-4-5 — 빠르고 값싼 편집과 대량 처리 단계.",
        ] },
        { type: "p", text: "키 하나로 모든 모델을 사용하므로, 계정이나 과금을 바꾸지 않고 확장에서 작업마다 모델을 전환할 수 있습니다." },
      ] },
    ],
    faq: [
      { q: "AI 코딩을 하려면 Cursor Pro가 필요한가요?", a: "아니요. Cline, Roo Code 같은 무료 VS Code 에이전트는 apitoken.sale Claude 키와 함께 작동합니다." },
      { q: "어떤 모델을 골라야 하나요?", a: "일상적인 코딩에는 claude-sonnet-5, 복잡한 작업에는 claude-opus-4-8을 사용하세요." },
    ],
  };
