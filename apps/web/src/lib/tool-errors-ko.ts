import type { ToolErrorTranslations } from "./tool-errors";

export const toolErrorsKo: ToolErrorTranslations = {
  ui: {
    eyebrow: "문제 해결",
    indexTitle: "AI 코딩 도구 오류 — Claude Code, Cursor, Codex, opencode, Cline, Zed",
    indexDescription:
      "개발자가 실제로 Claude API와 함께 사용하는 도구별 오류 페이지: Claude Code, Cursor, Codex CLI, opencode, Cline, Zed. 각 오류의 정확한 메시지, 원인, 해결 방법.",
    indexIntro:
      "문제가 발생한 도구를 선택하세요. 모든 페이지는 도구가 출력하는 텍스트를 그대로 인용하고, 무엇이 그 오류를 만들었는지 설명하며, 정상 동작까지의 가장 짧은 경로를 제시합니다. 상태 코드별로 정리된 API 수준 레퍼런스는 Claude API 오류 코드 페이지를 참고하세요.",
    errorsIn: "{tool} 오류",
    whatYouSee: "표시되는 내용",
    why: "발생 원인",
    how: "해결 방법",
    faqHeading: "자주 묻는 질문",
    alsoSearched: "관련 검색어",
    colError: "오류",
    colMeaning: "의미",
    backToTool: "{tool} 오류 전체",
    allTools: "모든 도구",
    fullReference: "Claude API 오류 코드",
    fullReferenceBlurb:
      "API 수준 레퍼런스: 모든 상태 코드와 응답 본문 원문을 도구가 아닌 코드 기준으로 정리했습니다.",
    setupGuide: "설정 가이드",
    ctaHeading: "잘못된 설정에 시간 낭비하지 마세요",
    ctaBody:
      "apiToken.sale은 표준 Anthropic API를 제공합니다 — 같은 모델, 같은 SDK, 하나의 선불 잔액. 도구의 base URL만 지정하면 이 페이지의 설정이 적힌 그대로 작동합니다.",
    ctaButton: "키 받기",
    ctaDocs: "API 문서",
  },
  index: {
    title: "AI 코딩 도구 오류 — Claude Code, Cursor, Codex, opencode, Cline, Zed",
    description:
      "개발자가 실제로 Claude API와 함께 사용하는 도구별 오류 페이지: Claude Code, Cursor, Codex CLI, opencode, Cline, Zed. 각 오류의 정확한 메시지, 원인, 해결 방법.",
    intro:
      "문제가 발생한 도구를 선택하세요. 모든 페이지는 도구가 출력하는 텍스트를 그대로 인용하고, 무엇이 그 오류를 만들었는지 설명하며, 정상 동작까지의 가장 짧은 경로를 제시합니다. 상태 코드별로 정리된 API 수준 레퍼런스는 Claude API 오류 코드 페이지를 참고하세요.",
  },
  tools: {
    "claude-code": {
      title: "Claude Code API 오류 — 429, 401, 529, Usage Limit 해결",
      description:
        "Claude Code가 출력하는 모든 오류의 해결책: API Error 429 rate_limit_error, 401 invalid x-api-key, 529 Overloaded, 사용량 한도 메시지. 정확한 출력, 원인, 해결 방법.",
      intro:
        "Claude Code는 API 실패를 \"API Error:\"로 시작하는 한 줄과 원본 응답 본문으로 표시하고, 구독 한도는 일반 문장으로 표시합니다. 아래 페이지는 각 형태의 정확한 텍스트, 발생 이유, 해결 방법을 다룹니다.",
    },
    cursor: {
      title: "Cursor 모델 프로바이더 오류 — 401, 429, Unable to Reach 해결",
      description:
        "커스텀 Anthropic API 키 사용 시 발생하는 Cursor 오류의 해결책: unable to reach model provider, 검증 시 invalid API key, 프로바이더의 401, 레이트 리밋. 각각의 원인과 해결 방법.",
      intro:
        "커스텀 Anthropic 키를 사용하면 Cursor는 프로바이더와 직접 통신하므로, 대부분의 실패는 키/base URL 설정 문제이거나 프로바이더의 응답이 그대로 전달된 것입니다. 아래 페이지에서 이 둘을 구분합니다.",
    },
    codex: {
      title: "Codex CLI 오류 — Missing OPENAI_API_KEY, config.toml, stream error 해결",
      description:
        "커스텀 모델 프로바이더 사용 시 Codex CLI 실패의 해결책: Missing OPENAI_API_KEY, config.toml 프로필 실수, auth.json 로그인 상태, stream error: unexpected status 401.",
      intro:
        "Codex CLI는 ~/.codex/config.toml을 읽고, 모델 프로바이더를 결정한 뒤, Responses API로 스트리밍합니다. 각 단계는 서로 다르게 실패합니다: 누락된 환경 변수, 잘못된 프로필, 만료된 로그인, 스트림 중간의 HTTP 오류. 아래 페이지는 이 순서대로 다룹니다.",
    },
    opencode: {
      title: "opencode 오류 — AI_APICallError, Model Not Found, 인증 해결",
      description:
        "커스텀 프로바이더에서 발생하는 opencode 실패의 해결책: AI_APICallError, 프로바이더 목록에 없는 모델, 인식되지 않는 API 키, 조용히 무시되는 이미지 첨부.",
      intro:
        "opencode는 Vercel AI SDK 위에 만들어져 프로바이더 실패가 AI_* 오류 클래스로 나타나며, models.dev 카탈로그에 없는 모델은 opencode.json에 기능을 직접 선언해야 합니다. 아래 페이지는 사용자에게 실제로 도달하는 실패들을 다룹니다.",
    },
    cline: {
      title: "Cline API 오류 — API Request Failed, 401, 429, 컨텍스트 한도 해결",
      description:
        "Anthropic 키 사용 시 발생하는 Cline 오류의 해결책: API Request Failed 배너, 401 invalid x-api-key, 429 rate_limit_error 재시도 루프, 컨텍스트 한도 400. 각각의 원인과 해결 방법.",
      intro:
        "Cline은 대부분의 실패를 \"API Request Failed\" 배너와 그 아래 프로바이더 응답으로 표시합니다. 배너 자체는 진단이 아닙니다 — 본문의 상태 코드와 error.type이 진단입니다. 아래 페이지는 각 본문을 해결책으로 연결합니다.",
    },
    zed: {
      title: "Zed Claude 오류 — 401, 429, 529, 커스텀 api_url 해결",
      description:
        "Anthropic 모델 사용 시 Zed 어시스턴트 오류의 해결책: 401 invalid x-api-key, 429 레이트 리밋, 529 Overloaded, 커스텀 api_url 설정과 /v1/v1 404 함정.",
      intro:
        "Zed의 어시스턴트는 Anthropic 응답을 거의 그대로 전달하므로, 보이는 오류 텍스트는 API 자체의 것입니다. Zed에 특화된 부분은 settings.json에서 키와 커스텀 api_url이 위치하는 곳이며, 지속되는 실패 대부분은 이 두 필드로 거슬러 올라갑니다.",
    },
  },
  entries: {
    // ——— Claude Code ———
    "claude-code/api-error-429": {
      title: "Claude Code API Error 429 (rate_limit_error) 원인과 해결 방법",
      description:
        "Claude Code는 분당 처리량이 소진되면 API Error: 429 rate_limit_error를 출력합니다. 이 한도가 실제로 무엇인지, 병렬 에이전트가 왜 이를 유발하는지, 어떻게 해결하는지 설명합니다.",
      causes: [
        "키 뒤에 있는 API 조직의 분당 토큰 또는 요청 한도를 초과했습니다. 메시지의 숫자는 해당 조직 고유의 한도이므로 계정마다 다릅니다.",
        "같은 키로 여러 Claude Code 세션이나 서브에이전트가 병렬로 실행 중입니다 — 각 세션이 매 턴마다 전체 컨텍스트를 다시 전송하므로 버스트가 보기보다 빠르게 쌓입니다.",
        "매우 큰 컨텍스트 하나만으로도 분당 토큰 예산을 초과할 수 있으며, 그래서 메시지가 대기뿐 아니라 프롬프트 단축도 제안하는 것입니다.",
        "첫 429를 유발한 버스트 위에 재시도가 겹겹이 쌓여, 상황을 해소하는 대신 키우고 있습니다.",
      ],
      fixes: [
        "1분 윈도우가 지나가길 기다리세요 — Claude Code는 자동으로 재시도하며 Retry-After를 준수합니다. 계속 반복되면 하나의 키를 공유하는 세션 수를 줄이세요.",
        "각 턴이 실어 나르는 양을 줄이세요: 비대해진 대화는 /compact 하거나, 긴 히스토리를 끌고 다니지 말고 새 세션을 시작하세요.",
        "\"Claude usage limit reached\"와 혼동하지 마세요 — 그것은 리셋 시각이 있는 구독 한도이지 분당 처리량이 아닙니다. 해결 방법이 다릅니다.",
        "키의 조직이 허용하는 것보다 지속적으로 더 많은 처리량이 필요하다면, 그것은 재시도 문제가 아니라 용량 협의 사안입니다 — 프로바이더와 상의하세요.",
      ],
      faq: [
        {
          q: "Claude Code의 API Error 429는 구독이 소진됐다는 뜻인가요?",
          a: "아니요. 429는 API 키의 분당 처리량입니다. 구독 소진은 리셋 시각과 함께 \"Claude usage limit reached\"로 표시됩니다.",
        },
        {
          q: "Claude Code는 429를 스스로 재시도하나요?",
          a: "네 — 자동으로 백오프하며 재시도합니다. 오류가 지속된다면 윈도우가 비워지는 속도보다 버스트가 다시 채워지는 속도가 빠른 것으로, 보통 같은 키를 쓰는 병렬 세션이 원인입니다.",
        },
        {
          q: "거대한 프롬프트 하나가 왜 단독으로 429를 일으키나요?",
          a: "레이트 리밋은 분당 토큰 수로 계산되며, 지나치게 큰 컨텍스트 하나가 요청 한 번에 그 분의 예산 전체를 써버릴 수 있습니다.",
        },
      ],
    },
    "claude-code/api-error-401": {
      title: "Claude Code API Error 401 invalid x-api-key 해결 방법",
      description:
        "Claude Code는 키가 통신 대상 엔드포인트에 도달하지 못하면 API Error: 401 invalid x-api-key를 출력합니다. 이를 유발하는 환경 변수 규칙과 해결 방법.",
      causes: [
        "ANTHROPIC_API_KEY와 ANTHROPIC_AUTH_TOKEN이 동시에 설정되어 있습니다 — 두 헤더가 모두 전송되어 요청이 거부됩니다. 빈 문자열도 설정된 것으로 간주됩니다. 커스텀 base URL을 쓸 때 가장 흔한 원인입니다.",
        "변수가 claude를 실행한 셸과 다른 셸에 설정되어 있습니다 — 한 터미널의 export는 다른 터미널에 존재하지 않으며, GUI 런처는 셸 프로필을 읽지 않습니다.",
        "ANTHROPIC_BASE_URL이 한 프로바이더를 가리키는데 키는 다른 곳에서 발급된 것입니다. 유효한 키라도 잘못된 엔드포인트로 보내면 401입니다.",
        "키가 폐기되었거나, 만료일이 설정된 키였다면 만료된 것입니다.",
      ],
      fixes: [
        "변수 하나만 고르고 다른 하나는 해제하세요: 이 게이트웨이에서는 ANTHROPIC_AUTH_TOKEN과 ANTHROPIC_BASE_URL을 사용하고, ANTHROPIC_API_KEY가 함께 export되어 있지 않은지 확인하세요.",
        "claude를 실행하는 바로 그 셸에서 확인하세요: 실행 직전에 변수의 앞부분 몇 글자를 출력해 보세요.",
        "대시보드에서 키가 활성 상태인지, base URL이 키 발급처와 일치하는지 확인하세요.",
      ],
      snippetLabel: "이 게이트웨이용 정상 동작 환경",
      faq: [
        {
          q: "커스텀 ANTHROPIC_BASE_URL을 설정한 직후 Claude Code가 왜 401을 반환하나요?",
          a: "보통 ANTHROPIC_API_KEY와 ANTHROPIC_AUTH_TOKEN이 동시에 설정되어 있거나, 키가 base URL이 가리키는 곳과 다른 엔드포인트의 것입니다. 변수 하나를 해제하고 키와 엔드포인트를 맞추세요.",
        },
        {
          q: "ANTHROPIC_API_KEY와 ANTHROPIC_AUTH_TOKEN 중 어느 것이 맞나요?",
          a: "ANTHROPIC_API_KEY는 x-api-key 헤더가 되고, ANTHROPIC_AUTH_TOKEN은 Authorization: Bearer가 됩니다. 정확히 하나만 사용하세요. 이 게이트웨이에서는 ANTHROPIC_AUTH_TOKEN이 문서화된 선택입니다.",
        },
        {
          q: "같은 키가 curl에서는 되는데 Claude Code에서는 안 됩니다 — 왜죠?",
          a: "claude를 실행하는 셸의 환경 상태가 다릅니다: 경합하는 변수, 오래된 값, 혹은 값이 아예 없는 경우입니다. 바로 그 셸의 환경을 확인하세요.",
        },
      ],
    },
    "claude-code/api-error-529": {
      title: "Claude Code API Error 529 Overloaded 의미와 대처 방법",
      description:
        "Claude Code는 업스트림 용량이 포화되면 API Error: 529 Overloaded를 출력합니다. 왜 당신의 요청 탓이 아닌지, 왜 몰려서 발생하는지, 실제로 도움이 되는 것은 무엇인지 설명합니다.",
      causes: [
        "업스트림 용량이 일시적으로 포화되었습니다. 529는 그 순간의 서비스 상태를 나타내는 것이지 당신의 요청을 나타내는 것이 아닙니다 — 페이로드의 어떤 것도 원인이 아닙니다.",
        "장애나 피크 시간대에 몰려서 발생합니다: 같은 요청이 보통 몇 분 뒤 아무 변경 없이 성공합니다.",
      ],
      fixes: [
        "Claude Code가 재시도하도록 두세요 — 자동으로 백오프합니다. 실행이 계속 죽는다면 같은 1분을 두드리지 말고 몇 분 기다리세요.",
        "무인 장기 실행에는 기계적인 단계에 더 작은 모델을 쓰는 편이 좋습니다: 일반적으로 경합이 덜해 용량 저하를 견딥니다.",
        "529가 오래 지속되면 설정을 바꾸기 전에 프로바이더의 상태 페이지를 확인하세요 — 당신의 설정이 원인인 경우는 거의 없습니다.",
      ],
      faq: [
        {
          q: "529는 429와 다른가요?",
          a: "네. 429는 당신 자신의 처리량 한도이고, 529는 업스트림 용량입니다. 백오프는 둘 다에 도움이 되지만, 사용량을 줄이는 것은 429에만 효과가 있습니다.",
        },
        {
          q: "제 프롬프트가 529를 일으켰나요?",
          a: "아니요. Overloaded는 서비스 측 상태입니다. 동일한 요청이 보통 용량이 회복되면 성공합니다.",
        },
        {
          q: "529가 보이면 모델을 바꿔야 하나요?",
          a: "지연에 민감한 작업이라면 일시적으로 더 작은 모델을 쓰는 것이 도움이 됩니다 — 경합이 덜하기 때문입니다. 그 외에는 기다리는 것으로 충분합니다.",
        },
      ],
    },
    "claude-code/usage-limit-reached": {
      title: "Claude Code의 Claude usage limit reached — 리셋 시각과 대안",
      description:
        "구독 요금제에서 Claude Code는 \"Claude usage limit reached. Your limit will reset at…\"을 표시합니다. 이 한도가 실제로 무엇을 세는지, 언제 리셋되는지, 종량제 API 접근은 어떻게 다른지 설명합니다.",
      causes: [
        "이것은 HTTP 오류가 아니라 Claude Pro 또는 Max 구독 한도입니다. Claude Code가 API 키가 아닌 구독으로 로그인되어 있을 때 나타납니다.",
        "한도는 롤링 윈도우로 적용됩니다 — 보통 5시간 세션 윈도우와 주간 상한의 조합이어서, 사용량이 많은 날에는 주가 끝나기 훨씬 전에 주간 할당량이 소진될 수 있습니다.",
        "긴 세션은 매 턴마다 전체 히스토리를 다시 전송하므로, 바쁜 대화 하나가 눈에 보이는 출력보다 훨씬 빠르게 할당량을 소비합니다.",
      ],
      fixes: [
        "명시된 리셋을 기다리세요 — 메시지에 시각이 적혀 있으며, 달력 경계가 아닌 롤링 윈도우입니다.",
        "각 턴이 실어 나르는 양을 줄이세요: 긴 대화는 /compact 하고, 무관한 작업들에 거대한 세션 하나를 끌고 다니지 마세요.",
        "리셋을 기다릴 수 없는 작업이라면, 종량제 API 접근은 요금제 할당량이 아닌 토큰 단위로 과금되므로 소진될 세션·주간 한도가 없습니다. 이것이 솔직한 차이입니다: 우회가 아니라 과금 방식의 차이입니다.",
      ],
      snippetLabel: "Claude Code를 구독에서 API 잔액으로 전환",
      faq: [
        {
          q: "Claude 사용량 한도는 언제 리셋되나요?",
          a: "메시지 자체에 적힌 시각에 리셋됩니다. 세션 윈도우는 롤링 방식(보통 5시간)으로 리셋되고, 주간 상한은 그것을 채운 사용 시점으로부터 일주일 뒤 리셋됩니다.",
        },
        {
          q: "이것이 API Error 429와 같은 건가요?",
          a: "아니요. 사용량 한도는 구독 할당량이고, 429는 분당 API 처리량입니다. 서로 다른 시스템에서 나오며 해결 방법도 다릅니다.",
        },
        {
          q: "API 접근에도 같은 주간 한도가 있나요?",
          a: "아니요. API 사용량은 잔액에 대해 토큰 단위로 측정되며, 분당 레이트 리밋은 있지만 세션·주간 할당량은 없습니다.",
        },
      ],
    },

    // ——— Cursor ———
    "cursor/unable-to-reach-model-provider": {
      title: "Cursor Unable to Reach the Model Provider 원인과 해결 방법",
      description:
        "Cursor는 HTTP 응답이 오기 전에 요청이 죽으면 모델 프로바이더에 연결할 수 없다고 보고합니다: 잘못된 base URL, 네트워크 경로, 또는 프로바이더 장애. 어느 경우인지 구분하는 방법.",
      causes: [
        "커스텀 base URL이 전송 계층에서 잘못되었습니다 — 호스트 오타, 스킴 실수, 어디로도 해석되지 않는 URL — 그래서 HTTP 응답이 아예 도착하지 않습니다.",
        "네트워크 경로가 차단되었습니다: Cursor와 엔드포인트 사이의 프록시, VPN, 방화벽이 연결을 끊습니다.",
        "프로바이더 자체가 잠시 다운되었습니다. 이 경우 당신 쪽에서 바뀐 것이 없고 오류는 저절로 사라집니다.",
        "프로바이더가 서비스하지 않는 뒤쪽 경로 조각과 함께 오버라이드 URL이 붙여넣어져, API 오류가 반환되기 전에 TLS 또는 HTTP 계층에서 실패합니다.",
      ],
      fixes: [
        "정확한 base URL을 Cursor 밖에서 curl로 테스트하세요. curl도 연결하지 못한다면 문제는 URL이나 네트워크이지 Cursor가 아닙니다.",
        "오버라이드는 오리진만 설정하고, /v1 경로는 클라이언트가 스스로 붙이게 하세요.",
        "curl은 되는데 Cursor는 안 된다면, Cursor가 터미널은 쓰지 않는 프록시나 VPN을 경유하는지 확인하세요.",
        "당신 쪽에서 바뀐 것이 없고 오류가 새로 생겼다면 몇 분 기다리세요 — 일시적인 프로바이더 장애가 정확히 이 메시지를 만듭니다.",
      ],
      snippetLabel: "엔드포인트 연결 가능 여부 확인",
      faq: [
        {
          q: "\"unable to reach model provider\"는 401이나 429와 같은 건가요?",
          a: "아니요. 이 메시지는 HTTP 응답이 아예 도착하지 않았다는 뜻입니다. 401이나 429는 프로바이더가 응답했고 요청을 거부했다는 뜻입니다 — 다른 계층, 다른 해결책입니다.",
        },
        {
          q: "어제까지 되던 Cursor가 아무 변경 없이 오늘 실패합니다 — 어떻게 하죠?",
          a: "그 패턴은 일시적 장애 또는 네트워크 경로 변화(VPN, 프록시, 캡티브 포털)입니다. 먼저 curl로 확인하세요; 이미 작동하던 설정을 다시 쓰지 마세요.",
        },
        {
          q: "Anthropic 오버라이드의 base URL은 무엇이어야 하나요?",
          a: "오리진만입니다 — 이 게이트웨이에서는 https://router.apitoken.sale입니다. /v1을 붙이지 마세요: 클라이언트가 API 경로를 스스로 추가하며, 중복된 경로는 실패합니다.",
        },
      ],
    },
    "cursor/invalid-api-key": {
      title: "Cursor Invalid API Key (Anthropic) — 검증이 실패하는 이유와 해결",
      description:
        "키와 base URL이 서로 맞지 않거나 오버라이드 필드가 여전히 기본 엔드포인트를 가리키면 Cursor가 검증에서 Anthropic 키를 거부합니다. 이를 해결하는 체크리스트.",
      causes: [
        "키는 커스텀 엔드포인트의 것인데 \"Override Anthropic Base URL\"이 꺼져 있거나 비어 있어, Cursor가 그 키를 본 적 없는 기본 api.anthropic.com에 대해 검증합니다.",
        "base URL은 설정되었지만 키가 공백과 함께 붙여넣어졌거나, 접두사가 빠졌거나, 아예 다른 프로바이더의 키 형식입니다.",
        "키가 폐기되었거나 만료되었습니다.",
        "오버라이드 URL에 경로 접미사가 포함되어, 검증 요청이 엔드포인트가 서비스하지 않는 URL로 갑니다.",
      ],
      fixes: [
        "키를 붙여넣기 전에 base URL 오버라이드를 켜고 키를 발급한 오리진으로 설정하세요 — 검증은 그 순간 활성화된 URL에 대해 실행됩니다.",
        "키를 앞뒤 공백 없이 붙여넣고, 접두사가 대시보드에 표시되는 것과 일치하는지 확인하세요.",
        "발급처의 대시보드에서 키가 활성 상태인지 확인한 뒤 Cursor에서 다시 검증하세요.",
      ],
      snippetLabel: "이 게이트웨이에 대해 검증되는 Cursor 설정",
      faq: [
        {
          q: "curl에서는 되는 키를 Cursor가 왜 invalid라고 하나요?",
          a: "Cursor는 그 시점에 설정된 base URL에 대해 검증합니다. 오버라이드가 꺼져 있으면 검증이 api.anthropic.com으로 가고, 게이트웨이 키는 거부됩니다. 오버라이드를 먼저 설정한 뒤 검증하세요.",
        },
        {
          q: "Cursor에서 커스텀 Anthropic 호환 키를 쓰려면 Anthropic 계정이 필요한가요?",
          a: "아니요. Anthropic 프로바이더 필드는 어떤 Anthropic 호환 엔드포인트든 받습니다: 오버라이드 URL을 발급처로 설정하고 그 발급처의 키를 붙여넣으세요.",
        },
        {
          q: "키가 검증된 후에는 어떤 모델을 쓸 수 있나요?",
          a: "오버라이드 뒤의 엔드포인트가 서비스하는 모델들입니다. 같은 base URL과 키로 GET /v1/models를 호출해 확인하세요.",
        },
      ],
    },
    "cursor/provider-returned-401": {
      title: "Cursor Request Failed With Status Code 401 — Anthropic 키 해결 방법",
      description:
        "Cursor 채팅 내부의 401은 프로바이더가 응답했고 자격 증명을 거부했다는 뜻입니다: 키와 base URL 불일치, 폐기된 키, 잘못된 헤더 도달. 해결 경로.",
      causes: [
        "키가 한 번 검증됐지만 이후 폐기되거나 만료되었습니다 — Cursor는 프로바이더가 401로 응답하기 시작할 때까지 그 키를 계속 사용합니다.",
        "키를 저장한 뒤 base URL 오버라이드가 변경되어, 저장된 키가 이제 그 키를 본 적 없는 엔드포인트로 갑니다.",
        "키 뒤의 계정이 정지되었거나 접근 권한이 회수되었습니다.",
      ],
      fixes: [
        "지금 이 순간의 조합을 다시 확인하세요: 오버라이드 URL과 키는 같은 발급처의 것이어야 합니다. 바뀐 쪽을 고치세요.",
        "정확히 그 키를 정확히 그 base URL에 대해 curl로 테스트하세요 — 한 번에 Cursor를 방정식에서 제외할 수 있습니다.",
        "curl도 401을 반환한다면 키 자체가 죽은 것입니다: 발급처 대시보드에서 상태를 확인하거나 재발급하세요.",
      ],
      snippetLabel: "Cursor 밖에서 재현하기",
      faq: [
        {
          q: "Cursor가 아까 키를 검증했는데 — 왜 지금 401인가요?",
          a: "검증은 스냅숏입니다. 이후에 폐기된 키나 이후에 변경된 base URL은 최초 검증이 통과했더라도 그 뒤의 모든 요청에서 401을 만듭니다.",
        },
        {
          q: "이건 Cursor의 버그인가요?",
          a: "거의 아닙니다. 401은 프로바이더의 답변이 그대로 전달된 것입니다. curl로 재현하세요: curl도 401을 받으면 자격 증명이 문제입니다.",
        },
        {
          q: "curl은 성공하는데 Cursor는 여전히 401이라면?",
          a: "그렇다면 Cursor가 당신이 생각하는 것과 다른 자격 증명을 보내고 있는 것입니다 — 설정을 다시 열고, 오버라이드 URL을 먼저 켠 상태에서 키를 다시 붙여넣으세요.",
        },
      ],
    },

    // ——— Codex CLI ———
    "codex/missing-openai-api-key": {
      title: "Codex CLI Missing OPENAI_API_KEY — 커스텀 프로바이더 설정 방법",
      description:
        "프로바이더가 기대하는 환경 변수가 설정되어 있지 않으면 Codex가 시작을 거부합니다. config.toml의 env_key 동작 방식, export가 셸 사이에서 사라지는 이유, 작동하는 프로필.",
      causes: [
        "프로바이더의 env_key가 지정한 환경 변수가 codex를 실행하는 셸에 설정되어 있지 않습니다. 커스텀 프로바이더에서는 변수 이름이 무엇이든 될 수 있으며 — 오류는 빠져 있는 그 변수의 이름을 표시합니다.",
        "키가 다른 터미널에서 export되었거나, 이 셸이 한 번도 source하지 않은 프로필 파일에 추가되었습니다.",
        "변수가 설정되어 있지만 비어 있으며, 이는 없는 것으로 간주됩니다.",
        "사용 중인 프로필이 생각과 다른 프로바이더를 가리켜, Codex가 당신이 export한 것과 다른 변수를 찾고 있습니다.",
      ],
      fixes: [
        "프로필에 프로바이더를 선언하고, 그 env_key가 지정한 정확한 변수를 같은 셸에서 codex 실행 전에 export하세요.",
        "실행 직전에 변수를 출력해 살아 있는지 확인하세요: export는 셸 단위이지 전역이 아닙니다.",
        "어떤 프로바이더 — 그리고 어떤 env_key — 가 활성인지 모호하지 않도록 프로필 이름을 명시해 codex를 실행하세요.",
      ],
      snippetLabel: "이 게이트웨이용 작동하는 프로필",
      faq: [
        {
          q: "Codex CLI를 실행하려면 OpenAI 계정 키가 필요한가요?",
          a: "아니요. 커스텀 모델 프로바이더에서는 env_key가 당신이 정한 아무 변수나 지정할 수 있고, 키는 그 프로바이더에서 옵니다 — OPENAI_API_KEY 자체는 기본 프로바이더의 변수일 뿐입니다.",
        },
        {
          q: "키를 export했는데도 Codex가 여전히 없다고 합니다 — 왜죠?",
          a: "export는 셸 단위입니다. codex가 다른 터미널, 멀티플렉서 패널, IDE 태스크에서 실행되면 그 환경은 당신의 export를 본 적이 없습니다. codex가 실제로 실행되는 곳에 설정하세요.",
        },
        {
          q: "키는 TOML과 셸 중 어디에 두어야 하나요?",
          a: "셸입니다. config.toml은 env_key를 통해 변수의 이름만 저장하므로 비밀 값이 설정 파일에 담기지 않습니다.",
        },
      ],
    },
    "codex/config-toml-error": {
      title: "Codex config.toml 오류 — 작동하는 model_providers 설정",
      description:
        "Codex는 잘못된 ~/.codex/config.toml을 로드하지 못합니다: 알 수 없는 프로바이더 이름, 잘못된 wire_api, 누락된 섹션. 커스텀 프로바이더에 필요한 정확한 구조.",
      causes: [
        "model_provider가 대응하는 [model_providers.<name>] 섹션이 없는 프로바이더를 지정합니다 — 참조와 섹션은 같은 식별자를 써야 합니다.",
        "TOML 문법 오류: 닫히지 않은 문자열, 섹션 헤더 오타, JSON에서 쉼표·중괄호째 붙여넣은 값.",
        "wire_api가 엔드포인트가 서비스하는 것과 맞지 않아, 요청이 잘못된 프로토콜 형태로 만들어집니다.",
        "편집한 파일이 Codex가 읽는 파일이 아닙니다 — --profile로 전달되는 프로필 파일에는 기대되는 특정 위치와 이름이 있습니다.",
      ],
      fixes: [
        "프로바이더 식별자를 양쪽에서 동일하게 유지하세요: model_provider = \"apitoken\"에는 [model_providers.apitoken]이 대응해야 합니다.",
        "Responses API 엔드포인트에는 wire_api = \"responses\"를 사용하세요 — 이 게이트웨이의 OpenAI 호환 표면은 Responses와 Chat Completions를 서비스합니다.",
        "파일이 진짜 TOML인지 검증하세요: 문자열은 따옴표로, 한 줄에 key = value 하나, 섹션 헤더는 대괄호로.",
        "codex --profile <name>을 실행하고 어떤 파일을 로드한다고 보고하는지 지켜보세요; 비슷하게 생긴 다른 파일이 아니라 그 파일을 고치세요.",
      ],
      snippetLabel: "최소한의 올바른 프로필",
      faq: [
        {
          q: "Codex가 왜 제 모델 프로바이더를 unknown이라고 하나요?",
          a: "model_provider 값은 같은 파일의 [model_providers.<name>] 섹션과 정확히 일치해야 합니다. 어느 쪽이든 오타가 있으면 조회가 깨집니다.",
        },
        {
          q: "wire_api는 responses여야 하나요, chat이어야 하나요?",
          a: "엔드포인트가 서비스하는 것을 따르세요. 이 게이트웨이의 OpenAI 호환 표면은 Responses API — wire_api = \"responses\" — 와 Chat Completions를 모두 받습니다.",
        },
        {
          q: "base_url에 /v1이 필요한가요?",
          a: "이 게이트웨이에 대한 Codex 프로필에서는 네: 문서화된 값은 문서에 실린 그대로 https://router.apitoken.sale/v1입니다.",
        },
      ],
    },
    "codex/auth-json-error": {
      title: "Codex auth.json / 로그인 오류 — API 키 인증으로 해결",
      description:
        "Codex 로그인 상태는 ~/.codex/auth.json에 있으며, 오래되거나 없는 파일은 로그인 프롬프트와 인증 실패를 만듭니다. 언제 재로그인해야 하고 언제 커스텀 프로바이더가 이를 완전히 건너뛰는지 설명합니다.",
      causes: [
        "auth.json이 없거나, 읽을 수 없거나, 오래되어 기본 프로바이더에 작동하는 로그인이 없습니다.",
        "로그인이 세션이 기대하는 것과 다른 계정 또는 요금제 상태의 것입니다 — 토큰 갱신이 더 이상 성공하지 않습니다.",
        "실패가 잘못 귀속된 경우입니다: 커스텀 프로바이더와 env_key를 쓸 때 ChatGPT 로그인 상태는 무관하며, 진짜 문제는 환경 변수나 프로필입니다.",
      ],
      fixes: [
        "기본 프로바이더라면 로그인 플로를 다시 실행해 auth.json이 새로 작성되게 하세요.",
        "커스텀 프로바이더에서는 auth.json이 관여하지 않습니다: 인증은 env_key 변수입니다. 프로필이 실제로 선택되어 있는지, 변수가 실행 중인 셸에 설정되어 있는지 확인하세요.",
        "두 경로를 머릿속에서 분리해 두세요 — 기본 프로바이더는 구독 로그인, 커스텀 프로바이더는 API 키. 한쪽의 오류는 다른 쪽에서 고쳐지지 않습니다.",
      ],
      faq: [
        {
          q: "Codex의 커스텀 프로바이더는 auth.json을 사용하나요?",
          a: "아니요. [model_providers.*] 항목은 env_key가 지정한 환경 변수로 인증합니다. auth.json은 기본 ChatGPT 로그인만 담습니다.",
        },
        {
          q: "Codex가 왜 계속 로그인하라고 하나요?",
          a: "저장된 로그인 상태를 갱신할 수 없기 때문입니다. 재로그인해 다시 쓰거나 — 커스텀 프로바이더를 쓰려던 것이라면 로그인이 필요 없도록 프로필을 명시적으로 선택하세요.",
        },
        {
          q: "ChatGPT 구독 없이 Codex를 실행할 수 있나요?",
          a: "네 — 커스텀 프로바이더 프로필과 그 프로바이더의 API 키가 환경에 있으면, Codex는 전적으로 API 키 인증으로 실행됩니다.",
        },
      ],
    },
    "codex/stream-error": {
      title: "Codex stream error: unexpected status 401/404 해결 방법",
      description:
        "Codex에서 스트림 중간의 HTTP 실패는 stream error: unexpected status로 출력됩니다. 여기서 401, 404, 429가 무엇을 뜻하는지, 그리고 curl로 재현해 고장 난 쪽을 찾는 방법.",
      causes: [
        "401 — env_key 변수가 설정되지 않았거나 비어 있거나, 그 키가 프로필의 base_url에 속하지 않습니다.",
        "404 — base_url이 와이어 프로토콜에 맞지 않습니다: /v1 누락, /v1/v1 중복, 또는 Responses API를 서비스하지 않는 호스트.",
        "429 — 키의 분당 처리량이 소진되었습니다; 스트림이 시작 전에 거부됩니다.",
        "프록시나 네트워크 장비가 SSE 스트림을 응답 중간에 끊으면 상태 코드 대신 연결 끊김 형태로 나타납니다.",
      ],
      fixes: [
        "프로필의 정확한 base_url에 /responses를 붙여 같은 키로 curl로 재현하세요 — 상태 코드가 어느 쪽이 고장인지 알려줍니다.",
        "401이면 키/엔드포인트 조합을 고치고, 404면 base_url을 문서화된 값으로 고치고, 429면 윈도우가 지나길 기다리며 병렬 실행을 줄이세요.",
        "상태 코드 없이 스트림 중간 끊김이 반복되면 VPN이나 프록시 없이 테스트하세요 — SSE는 간섭하는 미들박스의 첫 희생자입니다.",
      ],
      snippetLabel: "Codex 밖에서 재현하기",
      faq: [
        {
          q: "Codex의 stream error: unexpected status 401은 무슨 뜻인가요?",
          a: "엔드포인트가 스트림 시작 시점에 자격 증명을 거부했습니다. 프로필의 env_key 변수가 실행 중인 셸에 설정되어 있는지, 그 키가 프로필의 base_url에 속하는지 확인하세요.",
        },
        {
          q: "호스트가 분명히 맞는데 왜 404인가요?",
          a: "경로 실수입니다: 이 게이트웨이에서는 base_url에 /v1이 포함되어야 하며 중복되어서는 안 됩니다. Responses 경로는 Codex가 스스로 붙입니다.",
        },
        {
          q: "스트림이 상태 코드 없이 중간에 죽습니다 — API 문제인가요?",
          a: "보통 네트워크 경로입니다: SSE를 버퍼링하거나 종료시키는 프록시와 VPN. 엔드포인트를 탓하기 전에 깨끗한 연결에서 curl로 재현하세요.",
        },
      ],
    },

    // ——— opencode ———
    "opencode/ai-apicallerror": {
      title: "opencode AI_APICallError 원인과 해결 방법",
      description:
        "opencode는 프로바이더 실패를 Vercel AI SDK의 AI_APICallError로 표시합니다. 감싸진 상태 코드를 읽는 방법과 그 아래의 baseURL, 키, 모델을 고치는 방법.",
      causes: [
        "AI_APICallError는 진단이 아니라 래퍼입니다: AI SDK는 성공이 아닌 모든 HTTP 응답에 이를 던지며, 진짜 원인은 감싸진 상태 코드와 본문입니다.",
        "내부가 401 — 프로바이더의 apiKey 옵션이 잘못되었거나, {env:...} 플레이스홀더가 설정되지 않은 변수를 지정합니다.",
        "내부가 404 — baseURL이 프로바이더의 프로토콜에 맞지 않습니다: /v1 누락, /v1/v1 중복, 또는 엔드포인트가 서비스하지 않는 모델 id.",
        "내부가 429 또는 529 — 처리량 또는 업스트림 용량 문제로, 설정에는 잘못이 없습니다.",
      ],
      fixes: [
        "먼저 오류의 statusCode와 responseBody 필드를 읽으세요 — 프로바이더의 실제 답변이 담겨 있습니다.",
        "프로바이더 블록을 확인하세요: baseURL은 정확하게, apiKey는 opencode를 실행한 셸에 존재하는 환경 변수에서 해석되도록.",
        "opencode.json을 바꾼 뒤에는 opencode를 재시작하세요 — 설정은 시작 시점에 읽힙니다.",
      ],
      snippetLabel: "이 게이트웨이용 프로바이더 블록",
      faq: [
        {
          q: "AI_APICallError는 opencode의 버그인가요?",
          a: "아니요 — 프로바이더가 성공이 아닌 응답을 반환했다는 AI SDK의 보고입니다. 감싸진 상태 코드가 진짜 문제를 알려줍니다.",
        },
        {
          q: "opencode는 API 키를 어디서 읽나요?",
          a: "프로바이더의 options.apiKey에서 읽습니다. {env:NAME} 플레이스홀더를 쓰면, 그 변수는 opencode가 실행된 환경에 존재해야 합니다.",
        },
        {
          q: "opencode.json을 고쳤는데 아무 변화가 없습니다 — 왜죠?",
          a: "opencode는 설정을 시작 시점에 읽습니다. 변경할 때마다 재시작하세요.",
        },
      ],
    },
    "opencode/model-not-found": {
      title: "opencode Model Not Found — 커스텀 프로바이더 모델 선언 방법",
      description:
        "opencode는 자신이 아는 모델만 제공하며, 커스텀 프로바이더 모델은 models.dev 카탈로그에 없습니다. opencode.json에 모델을 선언해 표시되고 작동하게 만드는 방법.",
      causes: [
        "모델이 프로바이더의 models 맵에 선언되어 있지 않고, 커스텀 프로바이더 모델은 opencode가 참조하는 models.dev 카탈로그에도 없습니다 — 그래서 그 모델은 아예 제공되지 않습니다.",
        "선언된 모델 id가 엔드포인트가 서비스하는 것과 맞지 않습니다 — 오타나 퇴역한 id는 프로바이더의 404를 반환합니다.",
        "모델은 opencode.json에 추가됐지만 opencode를 재시작하지 않아 예전 설정이 여전히 활성 상태입니다.",
      ],
      fixes: [
        "각 모델을 opencode.json의 프로바이더 models 맵에 명시적으로 선언하고, id는 엔드포인트가 서비스하는 것과 정확히 일치시키세요.",
        "id를 추측하지 말고 GET /v1/models로 엔드포인트의 실제 모델을 나열하세요.",
        "변경 후 opencode를 재시작하세요.",
      ],
      snippetLabel: "모델을 명시적으로 선언하기",
      faq: [
        {
          q: "커스텀 프로바이더의 모델이 왜 opencode에 나타나지 않나요?",
          a: "models.dev 카탈로그 밖의 모델은 프로바이더의 models 맵에 직접 선언해야 합니다. 선언되지 않은 모델은 아예 제공되지 않습니다.",
        },
        {
          q: "선언할 정확한 모델 id를 어떻게 찾나요?",
          a: "당신의 키로 프로바이더의 GET /v1/models를 조회해 id를 그대로 복사하세요.",
        },
        {
          q: "모델은 선언했는데 요청이 404입니다 — 또 무엇이 있나요?",
          a: "opencode.json의 id는 엔드포인트의 id와 글자 하나까지 일치해야 하고, baseURL은 문서화된 값이어야 합니다. 둘 다 확인한 뒤 opencode를 재시작하세요.",
        },
      ],
    },
    "opencode/auth-config-error": {
      title: "opencode에서 API 키가 인식되지 않을 때 — auth와 {env} 플레이스홀더",
      description:
        "opencode는 커스텀 프로바이더를 options.apiKey — 보통 {env:...} 플레이스홀더 — 로 인증합니다. 변수가 조용히 빈 값으로 해석되어 요청이 401로 실패하는 이유.",
      causes: [
        "{env:NAME} 플레이스홀더가 opencode가 실행된 환경에 설정되지 않은 변수를 지정합니다 — 빈 값으로 해석되어 프로바이더는 키를 받지 못합니다.",
        "변수는 한 셸에서 export되었는데 opencode는 다른 곳에서 시작됩니다: 데스크톱 런처, 다른 터미널, 멀티플렉서 패널.",
        "키가 앞뒤 공백과 함께 opencode.json에 그대로 붙여넣어졌거나, baseURL과 다른 엔드포인트의 키입니다.",
      ],
      fixes: [
        "플레이스홀더에 적힌 정확한 변수를 같은 셸에서 export한 뒤 그 셸에서 opencode를 시작하세요.",
        "curl로 조합을 확인하세요: 설정의 baseURL과 변수의 키가 opencode 밖에서 먼저 성공해야 합니다.",
        "키를 파일에 붙여넣기보다 {env:...} 형태를 선호하세요 — 비밀 값이 dotfile과 버전 관리에 들어가지 않게 해줍니다.",
      ],
      snippetLabel: "키는 파일이 아닌 환경 변수로",
      faq: [
        {
          q: "opencode.json에 키를 지정했는데 왜 opencode가 API 키를 보내지 않나요?",
          a: "{env:NAME} 플레이스홀더는 시작 시점에 opencode 자신의 환경에서 해석됩니다. 그 셸이 변수를 export한 적이 없다면 키는 비어 있습니다.",
        },
        {
          q: "키를 opencode.json에 직접 붙여넣어도 안전한가요?",
          a: "작동은 하지만 env 플레이스홀더가 더 좋은 습관입니다: 설정 파일은 백업과 저장소로 흘러 들어가고, 붙여넣은 키도 함께 흘러갑니다.",
        },
        {
          q: "키와 URL 중 어느 쪽이 고장인지 어떻게 확인하나요?",
          a: "baseURL을 키와 함께 직접 curl 하세요. 401이면 키/엔드포인트 조합이 문제이고, 연결 오류면 URL이나 네트워크가 문제입니다.",
        },
      ],
    },
    "opencode/image-input-not-supported": {
      title: "opencode: This Model Does Not Support Image Input 해결 방법",
      description:
        "opencode는 모델의 modalities가 선언되어 있지 않으면 첨부된 이미지를 조용히 안내 문구로 대체합니다. 이미지 입력을 켜는 opencode.json 한 줄 선언.",
      causes: [
        "커스텀 프로바이더 모델은 models.dev 카탈로그에 없으므로, opencode는 모델의 실제 능력과 무관하게 텍스트 전용 기본 기능을 할당합니다.",
        "텍스트 전용 기능 상태에서 opencode는 첨부된 이미지를 \"this model does not support image input\"이라는 인라인 문구로 대체합니다; 프로바이더는 이미지를 아예 받지 못합니다.",
        "modalities는 선언했지만 그 뒤 opencode를 재시작하지 않았습니다.",
      ],
      fixes: [
        "opencode.json에서 해당 모델의 이미지 modality를 명시적으로 선언하고 opencode를 재시작하세요.",
        "modalities.input에 \"image\"가 들어가면, 붙여넣거나 첨부한 이미지가 표준 Chat Completions 이미지 파트로 전송되며, 이 게이트웨이는 이를 받습니다.",
      ],
      snippetLabel: "모델에 이미지 입력 선언하기",
      faq: [
        {
          q: "모델이 왜 제 이미지를 본 적 없는 것처럼 답하나요?",
          a: "실제로 본 적이 없습니다. 이미지 modality가 선언되어 있지 않으면 opencode가 첨부를 제거하고 그 자리에 텍스트 문구를 보냅니다 — 프로바이더에 도달하는 요청은 텍스트 전용입니다.",
        },
        {
          q: "이것은 프로바이더의 제한인가요?",
          a: "아니요 — 클라이언트 측 기능 게이트입니다. opencode.json에 modality를 선언하는 즉시 같은 모델이 이미지를 받습니다.",
        },
        {
          q: "다른 도구들도 이 선언이 필요한가요?",
          a: "기능 게이트가 있는 클라이언트만입니다. 와이어 계약은 일반적인 Chat Completions 이미지 파트이므로, OpenAI SDK처럼 게이트가 없는 클라이언트는 추가 작업이 필요 없습니다.",
        },
      ],
    },

    // ——— Cline ———
    "cline/api-request-failed": {
      title: "Cline API Request Failed — 진짜 오류를 진단하는 방법",
      description:
        "Cline의 API Request Failed 배너는 프로바이더 응답을 감싼 래퍼입니다. 그 아래의 상태 코드와 error.type을 읽고 실제 해결책으로 가는 방법.",
      causes: [
        "배너는 범용입니다: Cline은 실패한 모든 프로바이더 호출에 이를 표시합니다. 진단은 배너 텍스트가 아니라 그 아래의 상태 코드와 오류 본문입니다.",
        "아래가 401 — 키/엔드포인트 불일치 또는 폐기된 키.",
        "아래가 429 — 키의 분당 예산이며, Cline의 자동 재시도로 증폭됩니다.",
        "아래가 컨텍스트 메시지를 담은 400 — 대화와 max_tokens가 더 이상 모델의 윈도우에 들어가지 않습니다.",
      ],
      fixes: [
        "오류를 펼쳐 JSON 본문을 읽으세요. 모든 본문은 특정 페이지로 연결됩니다: 401은 잘못된 키 해결 페이지, 429는 레이트 리밋 페이지, 컨텍스트 400은 컨텍스트 한도 페이지.",
        "본문이 JSON이 아니라 연결 실패라면 네트워크/base URL 문제로 보고 curl로 엔드포인트를 확인하세요.",
        "반복 실패 시 무관한 설정을 이리저리 만지지 마세요 — 먼저 Cline 밖에서 재현한 뒤 정확히 한 가지만 바꾸세요.",
      ],
      faq: [
        {
          q: "Cline의 API Request Failed는 정확히 무슨 뜻인가요?",
          a: "프로바이더 호출이 성공하지 못했다는 것뿐입니다. 진짜 오류는 함께 표시되는 상태 코드와 본문입니다 — 그것부터 읽으세요.",
        },
        {
          q: "Cline이 반복해서 재시도하고 실패합니다 — retry를 계속 눌러야 하나요?",
          a: "아니요. 401과 400은 같은 요청이 영원히 실패합니다; 원인을 고치세요. 재시도는 일시적인 429/5xx 상황에만 도움이 되며, Cline은 이미 스스로 재시도합니다.",
        },
        {
          q: "Cline을 방정식에서 어떻게 제외하나요?",
          a: "Cline 설정의 base URL과 키로 같은 요청을 curl로 보내세요. curl의 결과가 문제가 설정인지 도구인지 알려줍니다.",
        },
      ],
    },
    "cline/invalid-api-key-401": {
      title: "Cline 401 invalid x-api-key (Anthropic) 해결 방법",
      description:
        "Anthropic 호환 엔드포인트가 키를 거부하면 Cline이 401 invalid x-api-key를 반환합니다: 잘못된 base URL 필드, 키의 공백, 폐기된 키. 해결 체크리스트.",
      causes: [
        "커스텀 base URL 옵션이 꺼져 있어, 키가 그것을 본 적 없는 기본 엔드포인트에 대해 검증됩니다.",
        "키가 앞뒤 공백과 함께 붙여넣어졌거나 잘렸습니다.",
        "base URL에 경로 접미사가 포함되어, 요청이 엔드포인트가 서비스하지 않는 URL로 갑니다.",
        "키가 폐기되었거나 만료되었습니다.",
      ],
      fixes: [
        "Cline의 Anthropic 프로바이더 설정에서 커스텀 base URL을 켜고 키를 발급한 오리진으로 설정하세요 — 이 게이트웨이에서는 /v1 접미사 없이 https://router.apitoken.sale입니다.",
        "키를 깨끗하게 다시 붙여넣고 발급처 대시보드에서 활성 상태인지 확인하세요.",
        "Cline으로 돌아가기 전에 curl로 조합을 검증하세요.",
      ],
      snippetLabel: "Cline 프로바이더 설정",
      faq: [
        {
          q: "다른 곳에서는 되는 키를 Cline이 왜 거부하나요?",
          a: "커스텀 base URL 체크박스가 꺼져 있으면 Cline은 키를 api.anthropic.com으로 보냅니다. base URL을 먼저 켜면 같은 키가 검증됩니다.",
        },
        {
          q: "Cline의 base URL에 /v1이 필요한가요?",
          a: "아니요 — 오리진만입니다. Cline의 SDK가 API 경로를 스스로 붙입니다; /v1 접미사는 중복 경로와 읽기 어려운 실패를 만듭니다.",
        },
        {
          q: "키가 작동한 뒤에는 어떤 모델을 선택할 수 있나요?",
          a: "엔드포인트가 서비스하는 모델들입니다. 같은 base URL과 키로 GET /v1/models를 호출해 확인하세요.",
        },
      ],
    },
    "cline/rate-limit-429": {
      title: "Cline 429 rate_limit_error — 재시도 루프 멈추는 방법",
      description:
        "Cline 에이전트 실행은 분당 토큰 예산을 빠르게 소진한 뒤 429 재시도 루프에 빠집니다. 에이전트 워크로드가 왜 레이트 리밋을 유발하는지, 실행을 통과시키는 방법.",
      causes: [
        "에이전트 루프는 토큰을 많이 씁니다: 매 단계마다 대화, 파일 컨텍스트, 도구 결과를 다시 전송하므로 활성 작업 하나가 1분 예산을 혼자 소진할 수 있습니다.",
        "키를 다른 도구나 팀원과 공유하고 있어 그들의 트래픽이 같은 윈도우를 채웁니다.",
        "포화된 윈도우 동안의 자동 재시도가 윈도우를 계속 포화 상태로 유지합니다.",
      ],
      fixes: [
        "429 한 번은 지나가게 두세요 — Cline이 백오프하며 재시도합니다. 실행이 429 루프에 빠지면 반복해서 재시작하지 말고 1분간 멈추세요.",
        "작업이 실어 나르는 컨텍스트를 줄이세요: 더 작은 파일 선택, 실행당 더 좁은 작업 범위.",
        "Cline에 전용 키를 주어 다른 도구의 버스트가 그 윈도우를 잠식하지 않게 하세요 — 지속적으로 더 많은 처리량이 필요하다면 한도와 싸우기보다 키 발급처와 상향을 협의하세요.",
      ],
      faq: [
        {
          q: "Cline은 왜 채팅 도구보다 훨씬 빨리 429에 도달하나요?",
          a: "에이전트 단계는 메시지 하나가 아닙니다 — 반복마다 히스토리, 파일 컨텍스트, 도구 출력을 다시 전송합니다. 분당 토큰 처리량이 채팅의 몇 배입니다.",
        },
        {
          q: "즉시 재시도하면 도움이 되나요?",
          a: "아니요 — 윈도우는 분 단위이며, 즉시 재시도는 윈도우를 계속 가득 채웁니다. 윈도우가 남은 시간 동안 백오프하는 것이 이를 비웁니다.",
        },
        {
          q: "이것은 Cline 자체의 한도인가요?",
          a: "아니요. 당신의 키를 쓰면 Cline에는 자체 과금이 없습니다; 429는 그 키 뒤 API 조직의 분당 한도입니다.",
        },
      ],
    },
    "cline/context-limit": {
      title: "Cline: input length and max_tokens exceed context limit 해결 방법",
      description:
        "400 input length and max_tokens exceed context limit는 Cline이 누적한 작업 컨텍스트와 출력 예약분이 더 이상 모델 윈도우에 들어가지 않을 때 나타납니다. 복구 방법.",
      causes: [
        "대화 히스토리 + 첨부 파일 + max_tokens 예약분이 모델의 컨텍스트 윈도우를 초과합니다 — 메시지의 두 숫자는 당신의 입력과 상한입니다.",
        "장기 실행 작업은 읽은 모든 파일과 도구 결과를 누적하며, 윈도우가 이미 가득 찰 때까지 아무것도 자동으로 버려지지 않습니다.",
        "큰 max_tokens 설정이 출력 공간을 예약하고, 그것이 입력 옆에 더 이상 들어가지 않습니다.",
      ],
      fixes: [
        "다음 작업 단위는 새 작업으로 시작하세요 — 거대한 작업 하나를 기능 전체에 걸쳐 끌고 가는 것이 윈도우를 채우는 원인입니다.",
        "작업이 담는 양을 줄이세요: 더 좁은 파일 언급, 참조로 대신할 수 있는 파일 전체를 붙여넣지 않기.",
        "모델에 더 큰 컨텍스트 윈도우 변형이 있다면 그것을 선택해 상한을 높일 수 있습니다; 아니라면 max_tokens를 올리기보다 입력을 줄이세요.",
      ],
      faq: [
        {
          q: "오류의 숫자들은 무슨 뜻인가요?",
          a: "입력 토큰 + max_tokens 예약분 대 모델의 윈도우입니다: 199999 + 8192 > 200000은 출력이 예약되기도 전에 입력만으로 윈도우가 거의 가득 찼다는 뜻입니다.",
        },
        {
          q: "전에는 다 되다가 왜 작업 중간에 이게 나타났나요?",
          a: "컨텍스트는 매 단계 누적됩니다. 작업이 그 단계에서 상한을 넘은 것은 그 단계가 특별해서가 아니라 한 단계 더 많았기 때문입니다.",
        },
        {
          q: "max_tokens를 낮추면 해결되나요?",
          a: "가끔 잠시는 됩니다 — 예약분이 줄어듭니다. 지속적인 해결은 입력을 줄이는 것입니다: 더 작은 작업, 다듬어진 컨텍스트.",
        },
      ],
    },

    // ——— Zed ———
    "zed/invalid-api-key": {
      title: "Zed Claude 401 invalid x-api-key — Anthropic 설정 해결 방법",
      description:
        "Zed의 어시스턴트는 Anthropic의 401 invalid x-api-key를 그대로 전달합니다. Zed 설정에서 키와 api_url이 어디에 있는지, 왜 같은 발급처의 것이어야 하는지 설명합니다.",
      causes: [
        "Zed의 Anthropic 프로바이더 설정의 키와 api_url이 서로 다른 발급처를 가리킵니다 — 게이트웨이 키가 기본 엔드포인트로 가거나, 그 반대입니다.",
        "키가 공백과 함께 붙여넣어졌거나 잘렸습니다.",
        "커스텀 api_url이 /v1 접미사와 함께 설정되어, 요청이 중복 경로를 치고 인증 전에 또는 인증 대신 실패합니다.",
        "키가 폐기되었거나 만료되었습니다.",
      ],
      fixes: [
        "language_models.anthropic.api_url을 키를 발급한 오리진으로 설정하고, 프로바이더 설정에 그 발급처의 키를 붙여넣으세요.",
        "api_url은 오리진만 유지하세요 — /v1 경로는 Zed가 스스로 붙입니다.",
        "다른 것을 바꾸기 전에 정확히 그 조합을 curl로 검증하세요.",
      ],
      snippetLabel: "이 게이트웨이용 Zed settings.json",
      faq: [
        {
          q: "Zed는 Anthropic base URL을 어디에 두나요?",
          a: "settings.json의 language_models.anthropic.api_url입니다. API 키 자체는 어시스턴트의 프로바이더 구성에서 입력합니다.",
        },
        {
          q: "curl에서는 되는 키가 Zed에서는 왜 실패하나요?",
          a: "Zed는 설정된 api_url이 무엇이든 거기로 키를 보냅니다. 그것이 curl 하는 URL과 다르다면 — 남아 있는 /v1을 포함해 — 키를 보는 엔드포인트는 발급처가 아닙니다.",
        },
        {
          q: "Zed에서 Claude를 쓰려면 Anthropic 계정이 필요한가요?",
          a: "아니요 — 어떤 Anthropic 호환 엔드포인트든 작동합니다: api_url을 발급처로 설정하고 그 키를 사용하세요.",
        },
      ],
    },
    "zed/rate-limit-429": {
      title: "Zed 어시스턴트 429 rate_limit_error 해결 방법",
      description:
        "Zed 어시스턴트의 429는 키 뒤 API 조직의 분당 한도입니다. 긴 스레드와 공유 키가 왜 이를 유발하는지, 무엇이 이를 해소하는지 설명합니다.",
      causes: [
        "키가 속한 조직의 분당 토큰 예산을 초과했습니다 — 긴 어시스턴트 스레드는 메시지마다 전체 히스토리를 다시 전송합니다.",
        "같은 키를 다른 도구가 동시에 사용하고 있어 그 트래픽이 윈도우를 공유합니다.",
        "큰 컨텍스트 첨부가 각 메시지가 실어 나르는 토큰을 배가시킵니다.",
      ],
      fixes: [
        "1분 윈도우가 지나가길 기다린 뒤 계속하세요 — 한 번의 429에는 설정 변경이 필요 없습니다.",
        "장기 실행 대화 하나를 늘리는 대신 새 작업에는 새 스레드를 시작하세요.",
        "현재 키가 도구들 사이에 공유되고 있다면 Zed에 전용 키를 주세요.",
      ],
      faq: [
        {
          q: "이것은 Zed의 한도인가요, 제 키의 한도인가요?",
          a: "키의 한도입니다. Zed는 자체 과금을 추가하지 않습니다; 429는 설정된 키 뒤의 API 조직에서 옵니다.",
        },
        {
          q: "긴 스레드는 왜 상황을 악화시키나요?",
          a: "각 메시지가 스레드 전체를 다시 전송합니다. 눈에 보이는 질문이 짧아도 계산되는 입력은 대화와 함께 커집니다.",
        },
        {
          q: "즉시 재시도하면 도움이 되나요?",
          a: "아니요 — 윈도우는 분 단위입니다. 즉시 재시도는 윈도우를 계속 포화 상태로 유지합니다; 잠깐의 멈춤이 이를 비웁니다.",
        },
      ],
    },
    "zed/overloaded-529": {
      title: "Zed: Claude의 529 Overloaded — 무엇을 해야 하나",
      description:
        "업스트림 용량이 포화되면 Zed는 Anthropic의 529 Overloaded를 그대로 표시합니다. 왜 당신의 설정이 원인이 아닌지, 지속되는 동안 무엇을 할지 설명합니다.",
      causes: [
        "업스트림 용량이 일시적으로 포화되었습니다. 529는 서비스를 나타내는 것이지 당신의 요청이나 Zed 설정을 나타내는 것이 아닙니다.",
        "장애와 피크 시간대에 몰려서 발생한 뒤, 당신 쪽 변경 없이 사라집니다.",
      ],
      fixes: [
        "잠시 멈춘 뒤 재시도하세요 — 동일한 요청이 보통 용량이 회복되면 성공합니다.",
        "기다릴 수 없는 작업이라면 스레드를 일시적으로 더 작은 모델로 전환하세요; 작은 모델은 일반적으로 경합이 덜합니다.",
        "설정 변경 충동을 참으세요: api_url과 키 변경은 용량 문제를 고칠 수 없으며 보통 진짜 오류를 하나 더 얹습니다.",
      ],
      faq: [
        {
          q: "제 Zed 설정이 529를 일으켰나요?",
          a: "아니요. Overloaded는 그대로 전달되는 서비스 측 상태입니다. 설정이 오류를 만들었다면 529가 아니라 401이나 404였을 것입니다.",
        },
        {
          q: "529 에피소드는 얼마나 지속되나요?",
          a: "보통 몇 분입니다. 지속된다면 설정을 편집하지 말고 프로바이더의 상태 페이지를 확인하세요.",
        },
        {
          q: "529는 429와 같은 건가요?",
          a: "아니요 — 429는 당신 자신의 처리량 한도이고, 529는 업스트림 용량입니다. 사용량을 줄이는 것은 429에만 효과가 있습니다.",
        },
      ],
    },
    "zed/api-url-config": {
      title: "Zed Anthropic api_url 설정 — 커스텀 base URL과 /v1/v1 404",
      description:
        "language_models.anthropic.api_url로 Zed의 Anthropic 프로바이더를 커스텀 엔드포인트로 지정하는 방법과, /v1을 붙이면 중복 경로로 404가 나는 이유.",
      causes: [
        "api_url이 /v1을 포함해 설정되었고 Zed가 API 경로를 스스로 붙입니다 — 요청이 존재하지 않는 /v1/v1/messages로 갑니다.",
        "api_url이 잘못된 설정 키 아래에 추가되었거나 오타가 있어, Zed가 아무 말 없이 기본 엔드포인트를 계속 사용합니다.",
        "엔드포인트는 맞는데 선택된 모델 id가 서비스되지 않는 것입니다 — 404의 또 다른 원인입니다.",
      ],
      fixes: [
        "api_url을 오리진만으로 설정하고 경로는 Zed가 만들게 하세요.",
        "설정을 settings.json의 정확히 language_models.anthropic.api_url 위치에 두세요 — 잘못 놓인 키는 오류를 내지 않고 그냥 아무것도 하지 않습니다.",
        "404 본문이 모델을 지목한다면 경로는 정상이고 모델 id가 문제입니다: 엔드포인트의 모델을 나열해 서비스되는 id를 고르세요.",
      ],
      snippetLabel: "올바른 settings.json",
      faq: [
        {
          q: "Zed의 api_url에 /v1을 포함해야 하나요?",
          a: "아니요. Zed가 /v1/messages를 스스로 붙입니다. /v1로 끝나는 api_url은 중복된 /v1/v1 경로와 404를 만듭니다.",
        },
        {
          q: "제 커스텀 api_url이 실제로 적용됐는지 어떻게 아나요?",
          a: "요청 하나만 의도적으로 깨뜨려 보세요(말이 안 되는 호스트) — 아무 변화가 없다면 Zed는 당신이 편집한 키를 읽고 있지 않은 것입니다; 설정 경로를 고치세요.",
        },
        {
          q: "경로가 맞는데도 여전히 404입니다 — 왜죠?",
          a: "404 본문이 보통 빠진 것의 이름을 알려줍니다. 모델을 지목한다면 엔드포인트가 서비스하는 id를 선택하세요; GET /v1/models로 나열할 수 있습니다.",
        },
      ],
    },
    "cursor/rate-limit-exceeded": {
      title: "Cursor에서 자체 Anthropic 키로 Rate Limit / 429 해결 방법",
      description:
        "커스텀 Anthropic 키를 쓰면 Cursor의 429는 Cursor 요금제 한도가 아니라 키 자체의 분당 한도입니다. 긴 채팅이 왜 이를 유발하는지, 루프를 멈추는 방법.",
      causes: [
        "키 뒤 조직의 분당 토큰 한도를 초과했습니다. 커스텀 키를 쓰면 Cursor 자체의 요금제 한도는 무관합니다 — 이것은 당신 키의 예산입니다.",
        "긴 채팅은 메시지마다 전체 대화와 첨부 컨텍스트를 다시 전송하므로, 바쁜 탭 하나가 1분 토큰 예산을 혼자 소진할 수 있습니다.",
        "같은 키를 다른 도구나 팀원과 공유하고 있어 그들의 트래픽이 같은 윈도우에 계산됩니다.",
        "첫 429 이후의 속사포 재시도가 윈도우를 계속 가득 채웁니다.",
      ],
      fixes: [
        "거대한 대화 하나를 늘리는 대신 새 작업에는 새 채팅을 시작하세요 — 메시지마다 다시 전송되는 컨텍스트가 예산을 잡아먹는 주범입니다.",
        "현재 키가 공유되고 있다면 Cursor에 전용 키를 주어 한 도구의 버스트가 다른 도구를 굶기지 않게 하세요.",
        "재시도 전에 1분 윈도우가 지나가길 기다리세요; 촘촘한 수동 재시도는 상황을 연장합니다.",
      ],
      faq: [
        {
          q: "이것은 Cursor의 fast-requests 한도인가요?",
          a: "아니요. 자체 키를 쓰면 요청이 Cursor의 요금제 과금을 완전히 우회합니다. 429는 당신 키 뒤의 API 조직에서 옵니다.",
        },
        {
          q: "긴 대화는 왜 429를 더 자주 만나나요?",
          a: "모든 메시지가 전체 히스토리와 첨부를 다시 전송합니다. 눈에 보이는 답변은 작아도 계산되는 입력 토큰은 대화와 함께 커집니다.",
        },
        {
          q: "Cursor 요금제를 업그레이드하면 도움이 되나요?",
          a: "이 오류에는 아닙니다. 한도는 API 키의 것입니다. 분당 사용량을 줄이거나, 키 공유를 멈추거나, 발급처와 더 높은 처리량을 협의하세요.",
        },
      ],
    },
  },
};
