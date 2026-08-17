import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude API로 스트리밍하기",
    h1: "Claude API에서 응답 스트리밍하기",
    description: "반응성 좋은 코딩 에이전트와 UI를 위해 apitoken.sale에서 Claude 응답을 스트리밍하는 방법. 동일한 Anthropic SSE 형식이며, 비스트리밍과 동일하게 과금됩니다.",
    keywords: ["claude api 스트리밍", "claude sse", "claude 응답 스트리밍", "anthropic 스트리밍 api", "claude api 실시간"],
    dek: "스트리밍은 토큰이 생성되는 대로 전송하므로 에이전트와 채팅 UI가 즉각적으로 느껴집니다. apitoken.sale은 표준 Anthropic 스트리밍 형식을 지원합니다.",
    sections: [
      { h2: "스트리밍하는 방법", blocks: [
        { type: "p", text: "요청에 \"stream\": true를 설정하세요(또는 SDK의 스트리밍 헬퍼를 사용하세요). 게이트웨이는 표준 Anthropic server-sent events를 반환합니다." },
        { type: "code", code: `curl https://router.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-sonnet-5",\n    "max_tokens": 1024,\n    "stream": true,\n    "messages": [{"role":"user","content":"Hello"}]\n  }'` },
      ] },
      { h2: "과금은 동일합니다", blocks: [
        { type: "p", text: "스트리밍 요청과 비스트리밍 요청은 입력 및 출력 토큰 단위로 동일하게 과금되므로, 스트리밍한다고 손해 볼 것이 없습니다." },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "스트리밍이 유리한 경우", blocks: [
        { type: "list", items: [
          "사용자가 답이 나타나는 것을 지켜보는 채팅과 코딩 UI.",
          "긴 생성 작업. 부분 출력을 일찍 렌더링하거나 처리할 수 있습니다.",
          "도구 호출이 나오는 즉시 멈추는 에이전트.",
        ] },
        { type: "p", text: "짧은 배치 작업에는 비스트리밍이 더 간단하며, 비용은 어느 쪽이든 동일합니다." },
      ] },
    ],
    faq: [
      { q: "apitoken.sale은 스트리밍을 지원하나요?", a: "네. 표준 Anthropic SSE 스트리밍 형식이 코딩 에이전트, IDE, 프로덕션 호출에서 작동합니다." },
      { q: "스트리밍은 비용이 더 드나요?", a: "아니요. 스트리밍 요청과 비스트리밍 요청은 토큰 단위로 동일하게 과금됩니다." },
    ],
  };
