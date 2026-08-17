import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude 3.5 대 Claude 4 — 무엇이 바뀌었나",
    h1: "Claude 3.5 대 Claude 4: 무엇이 바뀌었나",
    description: "Claude 3.5에서 현재 Claude 4 라인으로 옮기시나요? 무엇이 개선되었는지, 업데이트된 모델 ID, 그리고 apiToken.sale에서 베이스 URL 한 번 변경으로 전환하는 방법.",
    keywords: ["claude 3.5 vs 4", "claude 4 vs 3.5", "claude 모델 마이그레이션", "claude 모델 업그레이드", "새 claude 모델"],
    dek: "현재 Claude 라인은 추론과 코딩에서 3.5보다 확실히 향상되었습니다. 마이그레이션은 대부분 모델 ID 변경이며, 나머지는 그대로입니다.",
    sections: [
      { h2: "무엇이 개선되었나", blocks: [
        { type: "p", text: "Opus, Sonnet, Haiku 4 시리즈 모델은 동일한 Messages API를 유지하면서 에이전트형 코딩, 긴 컨텍스트 일관성, 복잡한 추론에서 3.5를 능가합니다." },
      ] },
      { h2: "마이그레이션 방법", blocks: [
        { type: "p", text: "모델 ID를 현재 것으로 교체하세요. 예를 들어 claude-opus-4-8, claude-sonnet-5, claude-haiku-4-5로 바꾸고 기존 요청 코드는 그대로 두면 됩니다. apiToken.sale에서는 동일한 키와 엔드포인트입니다." },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
    ],
    faq: [
      { q: "Claude 4가 3.5보다 훨씬 나은가요?", a: "네, 특히 코딩, 에이전트, 긴 컨텍스트 작업에서 그러하며, 동일한 API 형식을 사용합니다." },
      { q: "마이그레이션이 어렵나요?", a: "아니요 — 모델 ID를 업데이트하면(예: claude-sonnet-5로) 기존 Messages API 코드가 계속 작동합니다." },
    ],
  };
