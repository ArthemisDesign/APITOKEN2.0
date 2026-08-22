import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "apiToken.sale로 Codex CLI 설정하기 — GPT-5.6 프로필",
    h1: "apiToken.sale에서 Codex CLI 실행하기",
    description: "apiToken.sale OpenAI 호환 엔드포인트를 가리키는 이름 있는 model_providers 프로필로 Codex CLI를 설정하세요 — ChatGPT 계정 없이 선불 잔액으로 50% 통일 할인된 GPT-5.6 모델.",
    keywords: ["codex cli 설정", "codex config.toml", "codex 커스텀 모델 프로바이더", "codex api 키", "codex cli gpt-5.6", "codex responses api", "codex cli chatgpt 없이"],
    dek: "커스텀 모델 프로바이더를 지정하면 Codex CLI는 완전히 API 키 인증으로 실행됩니다. 하나의 TOML 프로필이 apiToken.sale을 가리키고, 선불 잔액이 모든 세션을 커버합니다 — ChatGPT 로그인 없이 공식 사용량 대비 50% 저렴합니다.",
    sections: [
      { h2: "프로필 만들기", blocks: [
        { type: "p", text: "다음을 ~/.codex/apitoken.config.toml로 저장하세요. 이름 있는 프로필은 기본 Codex 설정과 기존 ChatGPT 로그인을 건드리지 않습니다 — 실행할 때 명시적으로 선택합니다." },
        { type: "code", code: `# ~/.codex/apitoken.config.toml\nmodel = "gpt-5.6-sol"\nmodel_provider = "apitoken"\n\n[model_providers.apitoken]\nname = "apiToken.sale"\nbase_url = "https://router.apitoken.sale/v1"\nwire_api = "responses"\nenv_key = "APITOKEN_API_KEY"` },
        { type: "p", text: "env_key는 Codex가 키를 읽는 환경 변수의 이름을 지정합니다 — 시크릿은 셸에만 있고 TOML 파일에는 절대 들어가지 않습니다." },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작합니다 — 지원되는 Claude, GPT, Gemini, Kimi 모델에 유효하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "실행과 확인", blocks: [
        { type: "code", code: `export APITOKEN_API_KEY=sk-pool-•••\ncodex --profile apitoken` },
        { type: "list", items: [
          "항상 --profile apitoken을 명시적으로 전달해 어떤 프로바이더와 어떤 환경 변수가 활성인지 모호하지 않게 하세요.",
          "model 줄을 바꿔 프로젝트별로 모델을 전환하세요: 가장 어려운 작업은 gpt-5.6-sol, 일상적인 작업은 gpt-5.6-terra, 빠르고 저렴한 단계는 gpt-5.6-luna.",
          "Sol의 임시 공식 input/cached/cache write/output 요금은 2026-11-21까지(당일 포함) $4/$0.40/$5/$20이며 50% 할인 후 $2/$0.20/$2.50/$10입니다. 2026-11-22 UTC부터 표준 $5 input/$30 output 요금으로 돌아갑니다.",
          "같은 Bearer 키로 GET https://router.apitoken.sale/v1/models를 호출하면 현재 활성화된 모델 세트를 볼 수 있습니다 — 통합 카탈로그는 ID를 제공자별로 구분합니다(anthropic/*, openai/*, google/*).",
        ] },
        { type: "note", text: "wire_api = \"responses\"가 Codex 0.149의 유효한 값입니다. Codex 0.149는 Responses wire만 허용합니다. 게이트웨이는 다른 클라이언트를 위해 Chat Completions도 제공하지만, Codex에서 wire_api = \"chat\"은 유효하지 않습니다." },
      ] },
      { h2: "만날 수 있는 오류", blocks: [
        { type: "list", items: [
          "Missing APITOKEN_API_KEY — env_key가 가리키는 변수가 codex를 실행하는 셸에 export되어 있지 않습니다. 같은 셸(또는 셸 프로필)에서 export하세요.",
          "stream error: unexpected status 401 — 키가 잘못되었거나 폐기되었거나, base_url에서 /v1 접미사가 빠졌습니다. Codex 밖에서 curl로 재현해 어느 쪽이 깨졌는지 확인하세요.",
          "stream error: unexpected status 404 — 모델 ID가 활성화되어 있지 않습니다. 추측하지 말고 GET https://router.apitoken.sale/v1/models를 확인하세요.",
          "402 — 공유 선불 잔액을 충전해야 합니다. 기다린다고 해결되지 않습니다.",
        ] },
        { type: "link", text: "Codex 오류 전체 플레이북 — config.toml, auth.json, 스트림 오류", href: "/errors/codex" },
      ] },
    ],
    faq: [
      { q: "ChatGPT 계정이나 구독이 필요한가요?", a: "아니요. 커스텀 model_providers 프로필과 환경에 있는 프로바이더 API 키만 있으면 Codex는 완전히 API 키 인증으로 실행됩니다 — auth.json의 ChatGPT 로그인은 무관합니다." },
      { q: "기본 Codex 설정이 바뀌나요?", a: "아니요. 프로필은 자체 파일에 있으며 --profile apitoken을 전달할 때만 활성화됩니다. 기본 설정과 로그인은 그대로 유지됩니다." },
      { q: "할인이 Claude와 동일한가요?", a: "네. GPT-5.6 사용량은 공식 OpenAI 토큰 요금으로 측정되고 B2C 통일 50% 할인이 같은 선불 잔액에 적용됩니다." },
      { q: "Codex 0.149에서는 어떤 wire_api를 사용하나요?", a: "wire_api = \"responses\"를 사용하세요. Codex 0.149는 Responses wire만 허용합니다. 게이트웨이는 다른 클라이언트를 위해 Chat Completions도 제공하지만 Codex에서 wire_api = \"chat\"은 유효하지 않습니다." },
    ],
  };
