import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "API 키로 Claude Code 설정하기",
    h1: "apitoken.sale 키로 Claude Code 사용하기",
    description: "두 개의 환경 변수로 Claude Code를 apitoken.sale 키에 설정하고, 50% 통일 할인된 선불 잔액으로 모든 Claude 모델을 실행하세요.",
    keywords: ["claude code api 키", "claude code 설정", "claude code anthropic base url", "claude code 커스텀 키", "claude code 저렴하게"],
    dek: "Claude Code는 두 개의 환경 변수를 읽습니다. 이를 apitoken.sale로 지정하면 모든 기능을 유지하면서 할인된 선불 잔액으로 과금됩니다.",
    sections: [
      { h2: "두 개의 변수", blocks: [
        { type: "code", code: `export ANTHROPIC_BASE_URL=https://router.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••\n\n# then just run\nclaude` },
        { type: "p", text: "이게 설정의 전부입니다. 어려운 작업에는 claude-opus-4-8을, 일상적인 코딩에는 claude-sonnet-5를 사용하세요." },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "확인하고 모델 고르기", blocks: [
        { type: "p", text: "먼저 짧은 프롬프트를 실행해 키가 작동하는지 확인한 다음 기본 모델을 설정하세요. Claude Code가 인증 오류를 보고하면 두 환경 변수를 다시 확인하고 셸을 재시작해 변수가 export되도록 하세요." },
        { type: "list", items: [
          "일상적인 코딩: claude-sonnet-5.",
          "어려운 리팩터링과 긴 세션: claude-opus-4-8.",
          "대시보드에서 요청별 토큰 사용량을 확인해 소비를 추적하세요.",
        ] },
      ] },
    ],
    faq: [
      { q: "Claude Code를 apitoken.sale로 어떻게 지정하나요?", a: "ANTHROPIC_BASE_URL과 ANTHROPIC_API_KEY를 apitoken.sale 엔드포인트와 키로 설정한 뒤 claude를 실행하세요." },
      { q: "Claude Code의 모든 기능을 유지하나요?", a: "네. 구독에서 할인된 선불 사용량으로 과금만 바뀔 뿐입니다." },
    ],
  };
