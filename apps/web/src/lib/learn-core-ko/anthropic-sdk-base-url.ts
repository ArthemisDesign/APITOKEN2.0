import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "커스텀 Base URL로 Anthropic SDK 사용하기",
    h1: "Anthropic SDK를 apitoken.sale로 지정하기",
    description: "base_url을 router.apitoken.sale로 설정해 공식 Anthropic Python 및 TypeScript SDK를 apitoken.sale과 함께 사용하세요. 동일한 SDK, 동일한 코드, 더 낮은 토큰당 비용.",
    keywords: ["anthropic sdk base url", "anthropic python sdk 커스텀 엔드포인트", "claude sdk base url", "anthropic typescript sdk", "claude api sdk"],
    dek: "공식 Anthropic SDK는 base URL을 재정의할 수 있으므로, apitoken.sale로 전환하는 것은 한 줄만 바꾸면 됩니다. 모델 ID와 메시지 코드는 그대로 유지됩니다.",
    sections: [
      { h2: "Python", blocks: [
        { type: "code", code: `from anthropic import Anthropic\n\nclient = Anthropic(\n    base_url="https://router.apitoken.sale",\n    api_key="sk-pool-•••",\n)\nmsg = client.messages.create(\n    model="claude-opus-4-8",\n    max_tokens=1024,\n    messages=[{"role": "user", "content": "Hello"}],\n)` },
      ] },
      { h2: "TypeScript", blocks: [
        { type: "code", code: `import Anthropic from "@anthropic-ai/sdk";\n\nconst client = new Anthropic({\n  baseURL: "https://router.apitoken.sale",\n  apiKey: "sk-pool-•••",\n});\nconst msg = await client.messages.create({\n  model: "claude-opus-4-8",\n  max_tokens: 1024,\n  messages: [{ role: "user", content: "Hello" }],\n});` },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "전환이 제대로 되었는지 확인하기", blocks: [
        { type: "p", text: "base URL을 바꾼 뒤 요청을 한 번 보내 정상적인 Anthropic 응답이 오는지 확인하세요. 스트리밍, 도구 사용, 시스템 프롬프트 모두 api.anthropic.com과 완전히 동일하게 동작하며, 바뀐 것은 과금 엔드포인트뿐입니다." },
        { type: "list", items: [
          "401이 뜨면 키 또는 base URL이 잘못된 것입니다. 둘 다 다시 확인하세요.",
          "모델 ID는 그대로 유지하세요. 메시지 주변 코드는 전혀 바꿀 필요가 없습니다.",
          "대시보드에서 요청별 사용량을 확인해 소비와 할인이 맞는지 확인하세요.",
        ] },
      ] },
    ],
    faq: [
      { q: "공식 Anthropic SDK를 계속 사용할 수 있나요?", a: "네. base_url(Python) 또는 baseURL(TypeScript)만 apitoken.sale로 설정하면 나머지는 그대로입니다." },
      { q: "모델 ID가 바뀌나요?", a: "아니요. claude-opus-4-8, claude-sonnet-5처럼 동일한 모델 ID를 그대로 사용합니다." },
    ],
  };
