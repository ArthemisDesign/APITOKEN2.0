import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "2분 만에 Claude API 설정하기",
    h1: "2분 만에 Claude API 설정하기",
    description: "2분짜리 Claude API 퀵스타트: 키를 만들고, base URL을 router.apitoken.sale로 설정한 뒤, curl·Python·IDE로 첫 /v1/messages 요청을 보내세요.",
    keywords: ["claude api 퀵스타트", "claude api 설정", "claude api 첫 요청", "anthropic messages api", "claude api base url"],
    dek: "제로에서 작동하는 Claude API 호출까지 가장 빠른 길입니다. 아래 내용은 모두 표준 Anthropic Messages API를 사용하므로 기존 코드에 그대로 들어갑니다.",
    sections: [
      { h2: "1. 키 만들기", blocks: [ { type: "p", text: "가입하고 대시보드를 열어 키를 생성하세요. sk-pool-… 형태이며 지원되는 모든 모델에서 작동합니다." } ] },
      { h2: "2. 엔드포인트 설정하기", blocks: [
        { type: "p", text: "Anthropic 호환 클라이언트를 게이트웨이로 지정하세요:" },
        { type: "code", code: `Base URL:  https://router.apitoken.sale\nEndpoint:  POST /v1/messages\nHeaders:   x-api-key: sk-pool-•••\n           anthropic-version: 2023-06-01` },
      ] },
      { h2: "3. 첫 요청 보내기", blocks: [
        { type: "code", code: `curl https://router.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-opus-4-8",\n    "max_tokens": 1024,\n    "messages": [{"role":"user","content":"Hello"}]\n  }'` },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "흔한 첫 호출 오류", blocks: [
        { type: "list", items: [
          "401 Unauthorized — x-api-key가 누락되었거나 틀렸거나, base URL이 잘못되었습니다.",
          "400 Bad Request — 모델 ID와 max_tokens 설정 여부를 확인하세요.",
          "429 Too Many Requests — Retry-After를 준수하고 동시 요청 수를 낮추세요.",
          "402 / 잔액 부족 — 달러 단위 정수 금액으로 충전하세요.",
        ] },
      ] },
    ],
    faq: [
      { q: "어떤 base URL을 사용하나요?", a: "Anthropic 호환 도구에 https://router.apitoken.sale를 사용하고 /v1/messages로 요청을 보내세요. 레거시 호스트 https://api.apitoken.sale를 사용하는 기존 통합도 계속 작동합니다 — 통합 라우터는 새 설정에 권장되는 엔드포인트입니다." },
      { q: "어떤 인증 헤더가 필요한가요?", a: "공식 Anthropic API와 똑같이 키를 담은 x-api-key와 anthropic-version을 보내세요." },
    ],
  };
