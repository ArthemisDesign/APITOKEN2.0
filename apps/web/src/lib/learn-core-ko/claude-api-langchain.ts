import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "LangChain에서 Claude API 사용하기",
    h1: "LangChain에서 Claude API 사용하기",
    description: "apitoken.sale로 LangChain을 Claude에 연결하세요. ChatAnthropic을 router.apitoken.sale로 지정하면 모델 ID는 그대로, 토큰당 비용은 50% 저렴해집니다.",
    keywords: ["claude api langchain", "langchain anthropic", "langchain claude", "chatanthropic base url", "langchain claude api 키"],
    dek: "LangChain의 Anthropic 통합은 커스텀 API URL을 지원하므로, 두 줄만 바꾸면 체인과 에이전트가 apitoken.sale을 통해 Claude로 동작합니다. 같은 모델, 더 낮은 토큰 단가입니다.",
    sections: [
      { h2: "ChatAnthropic을 게이트웨이로 지정", blocks: [
        { type: "code", code: `from langchain_anthropic import ChatAnthropic\n\nllm = ChatAnthropic(\n    model="claude-opus-4-8",\n    anthropic_api_url="https://router.apitoken.sale",\n    anthropic_api_key="sk-pool-•••",\n)\nprint(llm.invoke("Hello").content)` },
        { type: "p", text: "통합은 이것이 전부입니다. 동일한 langchain-anthropic 패키지, 동일한 모델 ID, 동일한 스트리밍과 도구 호출 — 바뀌는 것은 엔드포인트와 가격뿐입니다." },
        { type: "note", text: "Google 또는 GitHub 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작합니다. 충전 전에 도구를 연결하고 실제 호출을 실행해 보기에 충분한 금액입니다." },
      ] },
      { h2: "또는 환경 변수로 구성", blocks: [
        { type: "code", code: `export ANTHROPIC_API_URL=https://router.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••` },
        { type: "p", text: "환경 변수를 설정하면 ChatAnthropic이 두 값을 자동으로 읽어오므로, 공유 코드베이스에서는 코드 수정이 전혀 필요 없습니다." },
      ] },
      { h2: "무엇이 작동하나", blocks: [
        { type: "list", items: [
          "체인, 에이전트, LangGraph 워크플로 — 프로토콜은 그대로입니다.",
          "표준 통합을 통한 스트리밍, 도구 호출, 구조화된 출력.",
          "지원되는 모든 Claude 모델(Opus, Sonnet, Haiku)을 하나의 키와 잔액으로.",
        ] },
      ] },
    ],
    faq: [
      { q: "LangChain이 커스텀 Claude API 엔드포인트를 지원하나요?", a: "네. ChatAnthropic은 anthropic_api_url(또는 ANTHROPIC_API_URL 환경 변수)을 받으므로, https://router.apitoken.sale로 지정하고 나머지는 그대로 두면 됩니다." },
      { q: "LangChain 에이전트와 도구 호출도 작동하나요?", a: "네 — 게이트웨이는 표준 Anthropic Messages API를 제공하므로 도구 호출, 스트리밍, LangGraph 에이전트가 공식 엔드포인트와 똑같이 동작합니다." },
      { q: "LangChain에서 어떤 모델을 쓸 수 있나요?", a: "지원되는 모든 Claude 모델 — claude-opus-4-8, claude-sonnet-5, claude-haiku-4-5 등 — 을 하나의 키와 선불 잔액으로 사용할 수 있습니다." },
    ],
  };
