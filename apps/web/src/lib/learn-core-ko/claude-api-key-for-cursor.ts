import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Cursor용 Claude API 키",
    h1: "Cursor에서 Claude API 키 사용하기",
    description: "apitoken.sale 키로 Cursor를 Claude에 연결하세요. Anthropic base URL을 router.apitoken.sale로 설정하고 키를 붙여넣은 뒤 모델을 골라 50% 통일 할인가로 코딩하세요.",
    keywords: ["cursor claude api 키", "cursor claude api", "cursor anthropic 키", "cursor에서 claude 사용", "cursor pro 없이"],
    dek: "Cursor는 자체 Anthropic 키를 가져올 수 있게 해 주므로, 번들 플랜 대신 할인된 선불 잔액으로 Cursor에서 Claude를 실행할 수 있습니다.",
    sections: [
      { h2: "세 단계 설정", blocks: [
        { type: "steps", items: [
          "Cursor → Settings → Models → Anthropic API를 여세요.",
          "base URL을 https://router.apitoken.sale로 설정하고 sk-pool-••• 키를 붙여넣으세요.",
          "claude-opus-4-8 같은 모델을 선택하고 코딩을 시작하세요.",
        ] },
      ] },
      { h2: "구성", blocks: [
        { type: "code", code: `# Cursor → Settings → Models → Anthropic API\nBase URL : https://router.apitoken.sale\nAPI key  : sk-pool-•••\nModel    : claude-opus-4-8` },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "문제 해결", blocks: [
        { type: "list", items: [
          "Cursor가 키를 무시함: OpenAI가 아니라 Anthropic 제공자를 편집했는지 확인하세요.",
          "모델을 찾을 수 없음: claude-opus-4-8 같은 최신 모델 ID를 설정하세요.",
          "401: base URL과 키가 전부 붙여넣어졌는지 다시 확인하세요.",
        ] },
        { type: "p", text: "연결되면 지원되는 모든 Claude 모델을 동일한 키와 잔액으로 사용할 수 있습니다." },
      ] },
      { h2: "어떤 언어에서든 Cursor에서 쓰는 Claude API 키", blocks: [
        { type: "p", text: "키는 언어에 구애받지 않습니다. Cursor는 Python, JavaScript, TypeScript, Go, Rust 등 어떤 프로젝트에서든, Windows·macOS·Linux에서 이 키를 사용합니다. 설정하는 것은 모델 제공자이지 언어가 아닙니다." },
      ] },
    ],
    faq: [
      { q: "Cursor에서 내 Claude 키를 사용할 수 있나요?", a: "네. Cursor의 Anthropic 제공자는 커스텀 base URL과 키를 받아들이므로 apitoken.sale로 지정할 수 있습니다." },
      { q: "Cursor Pro가 여전히 필요한가요?", a: "자체 API 키와 잔액으로 Claude를 실행할 수 있습니다. Cursor 자체 플랜이 필요한 기능은 모델 제공자와 별개입니다." },
      { q: "Claude API 키가 Windows와 Mac의 Cursor에서 작동하나요?", a: "네 — Anthropic 제공자 설정은 Windows, macOS, Linux에서 동일합니다." },
    ],
  };
