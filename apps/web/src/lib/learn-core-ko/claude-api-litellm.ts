import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "LiteLLM에서 Claude API 사용하기",
    h1: "LiteLLM에서 Claude API 사용하기",
    description: "apitoken.sale로 LiteLLM을 Claude에 라우팅하세요. litellm_params 또는 프록시 설정에서 api_base를 router.apitoken.sale로 지정하면 토큰당 50% 저렴합니다.",
    keywords: ["claude api litellm", "litellm anthropic", "litellm claude", "litellm api_base anthropic", "litellm 프록시 claude"],
    dek: "LiteLLM은 Anthropic을 네이티브로 지원하고 모델별 엔드포인트 재정의를 허용하므로, 설정 한 줄로 모든 Claude 트래픽이 할인 게이트웨이를 지나갑니다.",
    sections: [
      { h2: "SDK 직접 호출", blocks: [
        { type: "code", code: `import litellm\n\nresponse = litellm.completion(\n    model="anthropic/claude-opus-4-8",\n    api_base="https://router.apitoken.sale",\n    api_key="sk-pool-•••",\n    messages=[{"role": "user", "content": "Hello"}],\n)` },
        { type: "note", text: "Google 또는 GitHub 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작합니다. 충전 전에 도구를 연결하고 실제 호출을 실행해 보기에 충분한 금액입니다." },
      ] },
      { h2: "LiteLLM 프록시 설정", blocks: [
        { type: "code", code: `# config.yaml\nmodel_list:\n  - model_name: claude-opus-4-8\n    litellm_params:\n      model: anthropic/claude-opus-4-8\n      api_base: https://router.apitoken.sale\n      api_key: sk-pool-•••` },
        { type: "p", text: "이 설정으로 프록시를 실행하면 LiteLLM 게이트웨이의 모든 클라이언트가 투명하게 할인된 Claude 엔드포인트를 사용합니다. 여러 서비스가 하나의 라우팅 계층을 공유할 때 유용합니다." },
      ] },
      { h2: "왜 LiteLLM으로 Claude를 이곳에 라우팅하나", blocks: [
        { type: "list", items: [
          "모든 서비스를 저렴한 엔드포인트로 전환하는 단일 지점.",
          "이미 쓰던 anthropic/ 모델 접두사와 파라미터 그대로.",
          "apitoken.sale 대시보드에서 키별 지출을 토큰 단위로 추적.",
        ] },
      ] },
    ],
    faq: [
      { q: "LiteLLM이 커스텀 Anthropic api_base를 지원하나요?", a: "네 — litellm.completion()이나 프록시 설정의 litellm_params에 api_base를 전달하면 LiteLLM이 Anthropic 형식 요청을 https://router.apitoken.sale로 보냅니다." },
      { q: "anthropic/ 모델 접두사는 유지하나요?", a: "네. anthropic/claude-opus-4-8(또는 지원되는 모든 모델)을 사용해 LiteLLM이 Anthropic 프로토콜을 적용하게 하세요. 바뀌는 것은 엔드포인트와 키뿐입니다." },
      { q: "LiteLLM 기반 도구에도 적용되나요?", a: "네 — LiteLLM을 거치는 모든 것(많은 코딩 에이전트 포함)이 같은 설정에서 할인 엔드포인트를 물려받습니다." },
    ],
  };
