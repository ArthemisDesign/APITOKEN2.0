import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "OpenAI 호환 API 빠른 시작 — 하나의 키로 GPT-5.6",
    h1: "OpenAI 호환 API 빠른 시작: Responses와 Chat Completions",
    description: "apiToken.sale의 OpenAI 호환 API로 GPT-5.6 모델을 실행하세요 — SSE 스트리밍을 지원하는 Responses와 Chat Completions, Claude와 공유하는 하나의 sk-pool 키와 잔액, 50% 통일 할인.",
    keywords: ["openai 호환 api", "gpt-5.6 api", "responses api", "chat completions 커스텀 base url", "openai sdk base_url", "gpt api 키", "gpt-5.6 가격"],
    dek: "sk-pool 키는 Claude 전용이 아닙니다. 같은 키와 선불 잔액으로 OpenAI 호환 엔드포인트를 통해 GPT-5 라인업을 사용할 수 있습니다 — 표준 Responses 및 Chat Completions 호출, 공식 OpenAI SDK, SSE 스트리밍, 동일한 50% 통일 할인.",
    sections: [
      { h2: "첫 GPT 호출까지 세 단계", blocks: [
        { type: "steps", items: [
          "무료 계정을 만들고 API 키 하나를 발급받으세요(sk-pool-… 형태) — 이 키는 이미 Claude 모델도 커버합니다.",
          "클라이언트를 https://router.apitoken.sale/v1 로 지정하고 Authorization: Bearer로 인증하세요 — x-api-key가 아닙니다. 그 헤더는 Anthropic 서피스 전용입니다.",
          "GET https://router.apitoken.sale/v1/models로 활성화된 모델을 확인하세요 — 통합 카탈로그는 ID를 제공자별로 구분합니다(anthropic/*, openai/*, google/*) — 그런 다음 Responses 요청을 보내세요.",
        ] },
        { type: "code", code: `curl https://router.apitoken.sale/v1/responses \\\n  -H "Authorization: Bearer $APITOKEN_API_KEY" \\\n  -H "Content-Type: application/json" \\\n  -d '{\n    "model": "gpt-5.6-sol",\n    "input": "Reply with exactly: connected"\n  }'` },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작합니다 — 지원되는 Claude, GPT, Gemini, Kimi 모델에 유효하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "공식 OpenAI SDK 사용", blocks: [
        { type: "p", text: "공식 SDK는 그대로 동작합니다 — base_url과 키만 바뀝니다. 프로덕션에서는 키를 서버 측 환경 변수에 보관하세요." },
        { type: "code", code: `import os\nfrom openai import OpenAI\n\nclient = OpenAI(\n    api_key=os.environ["APITOKEN_API_KEY"],\n    base_url="https://router.apitoken.sale/v1",\n)\n\nresponse = client.responses.create(\n    model="gpt-5.6-sol",\n    input="Reply with exactly: connected",\n)\nprint(response.output_text)` },
        { type: "p", text: "클라이언트가 필요로 한다면 같은 호스트에서 Chat Completions도 제공됩니다 — 모델 ID와 키는 동일합니다." },
        { type: "code", code: `completion = client.chat.completions.create(\n    model="gpt-5.6-sol",\n    messages=[{"role": "user", "content": "Hello"}],\n)\nprint(completion.choices[0].message.content)` },
      ] },
      { h2: "사용 가능한 GPT 모델", blocks: [
        { type: "p", text: "제공되는 모델 세트는 엔진에 고정되어 가격이 매겨집니다. GET https://router.apitoken.sale/v1/models가 항상 최신 답변입니다. 현재 세 가지 GPT-5.6 티어와 두 가지 이전 세대 모델을 제공합니다:" },
        { type: "table", headers: ["모델 ID", "티어", "공식 입력 / 출력($ / 1M)", "캐시 입력"], rows: [
          ["gpt-5.6-sol(별칭: gpt-5.6)", "플래그십", "$4 / $20 (임시)", "$0.40"],
          ["gpt-5.6-terra", "밸런스", "$2 / $12", "$0.20"],
          ["gpt-5.6-luna", "고속", "$0.20 / $1.20", "$0.02"],
          ["gpt-5.5", "이전 세대 플래그십", "$5 / $30", "$0.50"],
          ["gpt-5.4", "이전 세대 밸런스", "$2.50 / $15", "$0.25"],
        ] },
        { type: "list", items: [
          "Sol의 임시 공식 input/cached/cache write/output 요금은 2026-11-21까지(당일 포함) $4/$0.40/$5/$20이며 50% 할인 후 $2/$0.20/$2.50/$10입니다. 2026-11-22 UTC부터 표준 $5 input/$30 output 요금으로 돌아갑니다.",
          "추론 강도는 요청마다 조절할 수 있습니다 — 모든 모델에서 none부터 xhigh까지, GPT-5.6 라인업은 max까지 지원합니다.",
          "모든 모델이 텍스트와 이미지 입력을 받고 Responses와 Chat Completions 모두에서 SSE로 스트리밍합니다.",
          "reasoning token은 output으로 청구되며, 프로모션 기간 Sol의 공식 output 요금은 100만 token당 $20입니다.",
          "272K 입력 토큰을 초과하는 요청은 OpenAI 장문 컨텍스트 요금으로 청구됩니다: 전체 요청에 입력 2배, 출력 1.5배. 프로모션 Sol에서 270K input + 2K output은 공식 $1.12이고, 273K input + 2K output은 $2.244입니다.",
          "B2C 할인은 Claude 사용량과 정확히 동일하게 적용됩니다 — 하나의 잔액, 하나의 요율, 공식 사용량 대비 50% 할인.",
        ] },
        { type: "link", text: "모델별 전체 사양과 할인 가격", href: "/models" },
      ] },
      { h2: "엔드포인트가 커버하는 범위", blocks: [
        { type: "p", text: "이것은 OpenAI Platform이 아닌 독립적인 OpenAI 호환 서비스입니다. 모델 카탈로그, 스트리밍 Responses와 Chat Completions뿐 아니라 GPT Image 2 전용 이미지 생성 및 편집 routes도 제공합니다. audio, files, realtime, assistants, batch, fine-tuning endpoints는 제공되지 않습니다." },
        { type: "note", text: "오류는 OpenAI 봉투로 반환됩니다 — {\"error\":{\"message\",\"type\",\"param\",\"code\"}}. 401은 키 또는 인증 헤더 오류(Bearer 사용, x-api-key 아님), 402는 공유 선불 잔액 충전 필요, 404는 모델 ID 미활성화를 의미합니다 — GET https://router.apitoken.sale/v1/models를 확인하세요." },
      ] },
    ],
    faq: [
      { q: "같은 키를 GPT 외의 모델에도 쓸 수 있나요?", a: "네. 하나의 sk-pool 키와 잔액으로 지원되는 Claude, Gemini, Kimi도 이용할 수 있습니다. 각 프로바이더에 맞는 프로토콜과 인증 헤더를 사용하세요." },
      { q: "OpenAI 호환 엔드포인트는 어떤 인증 헤더를 쓰나요?", a: "Authorization: Bearer sk-pool-… 입니다. x-api-key 헤더는 Anthropic 서피스 전용입니다 — OpenAI 엔드포인트에내면 401이 반환됩니다." },
      { q: "Responses와 Chat Completions 중 무엇을 쓰나요?", a: "둘 다 SSE 스트리밍으로 제공됩니다. 새 코드와 공식 SDK에는 Responses를, 클래식 형태를 기대하는 클라이언트와 프레임워크에는 Chat Completions를 사용하세요." },
      { q: "GPT 사용량은 어떻게 과금되나요?", a: "캐시 입력과 장문 컨텍스트 가격을 포함한 공식 OpenAI 요금으로 토큰당 과금된 후, 50% B2C 통일 할인이 차감되어 선불 잔액에서 청구됩니다 — Claude 사용량과 정확히 같습니다." },
    ],
  };
