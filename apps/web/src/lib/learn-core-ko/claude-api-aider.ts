import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Aider에서 Claude API 사용하기",
    h1: "Aider에서 Claude API 사용하기",
    description: "apitoken.sale로 Claude에서 Aider를 실행하세요. ANTHROPIC_API_BASE와 키를 내보내고 Claude 모델을 골라 50% 통일 할인가로 터미널 페어 프로그래밍을 하세요.",
    keywords: ["claude api aider", "aider anthropic", "aider claude", "aider anthropic api base", "aider claude api 키"],
    dek: "Aider는 긴 세션에서 토큰을 빠르게 소모하는 터미널 페어 프로그래머입니다. 환경 변수 두 개로 할인 게이트웨이를 가리키게 하고 워크플로는 그대로 유지하세요.",
    sections: [
      { h2: "환경 변수 두 개", blocks: [
        { type: "code", code: `export ANTHROPIC_API_KEY=sk-pool-•••\nexport ANTHROPIC_API_BASE=https://router.apitoken.sale\n\naider --model anthropic/claude-opus-4-8` },
        { type: "p", text: "Aider는 내부적으로 LiteLLM을 통해 Anthropic 트래픽을 라우팅하며, LiteLLM은 ANTHROPIC_API_BASE를 인식합니다. 설정 파일이 필요 없습니다." },
        { type: "note", text: "Google 또는 GitHub 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작합니다. 충전 전에 도구를 연결하고 실제 호출을 실행해 보기에 충분한 금액입니다." },
      ] },
      { h2: "Aider용 모델 고르기", blocks: [
        { type: "list", items: [
          "anthropic/claude-opus-4-8 — 가장 어려운 리팩터링과 긴 에이전트 편집.",
          "anthropic/claude-sonnet-5 — 일상 기본값; Opus에 가까운 코딩 품질.",
          "anthropic/claude-haiku-4-5 — 빠른 수정과 저렴한 실험.",
        ] },
        { type: "p", text: "긴 Aider 세션이야말로 토큰 할인이 누적되는 곳입니다. 저장소 맵, diff, 다중 파일 편집이 모두 입력과 출력으로 과금됩니다." },
      ] },
    ],
    faq: [
      { q: "Aider가 커스텀 Claude 엔드포인트를 지원하나요?", a: "네. Aider는 Anthropic 모델에 LiteLLM을 사용하고, LiteLLM은 ANTHROPIC_API_BASE 환경 변수를 인식합니다. https://router.apitoken.sale로 설정하고 평소처럼 Aider를 시작하세요." },
      { q: "Aider에서 어떤 Claude 모델이 가장 좋나요?", a: "대부분의 코딩에는 claude-sonnet-5가 최선의 기본값이고, 가장 어려운 다중 파일 작업은 claude-opus-4-8로 전환하세요. 둘 다 같은 키에서 동작합니다." },
      { q: "긴 Aider 세션은 얼마나 저렴해지나요?", a: "모든 요청이 공식 토큰 요율에서 50% 통일 할인을 뺀 금액으로 과금되므로, 직접 연결로 $10짜리 세션이 여기서는 $5입니다." },
    ],
  };
