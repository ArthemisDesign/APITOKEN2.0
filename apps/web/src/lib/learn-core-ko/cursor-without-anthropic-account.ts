import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Anthropic 계정 없이 Cursor에서 Claude 사용하기",
    h1: "Anthropic 계정 없이 Cursor에서 Claude 실행하기",
    description: "Anthropic 계정이 없나요? 대신 apitoken.sale 키로 Cursor에서 Claude를 사용하세요. 즉시 접근, 카드 또는 암호화폐 결제, 공식 API 요율 대비 50% 통일 할인.",
    keywords: ["anthropic 계정 없이 cursor", "anthropic 없이 cursor claude", "cursor claude api 키", "anthropic 계정 없이 claude 사용"],
    dek: "Anthropic 계정을 만들 수 없거나 만들고 싶지 않다면, apitoken.sale이 Cursor가 Anthropic 제공자로 받아들이는 자체 키를 발급합니다.",
    sections: [
      { h2: "왜 작동하는가", blocks: [
        { type: "p", text: "Cursor는 Anthropic Messages API와 통신합니다. apitoken.sale은 바로 그 API를 그대로 노출하므로, Cursor는 차이를 알 수 없습니다. 그저 여러분의 키와 base URL을 사용할 뿐입니다." },
      ] },
      { h2: "설정하기", blocks: [
        { type: "code", code: `# Cursor → Settings → Models → Anthropic API\nBase URL : https://router.apitoken.sale\nAPI key  : sk-pool-•••\nModel    : claude-opus-4-8` },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "그대로 유지되는 것", blocks: [
        { type: "list", items: [
          "전체 Claude 라인업(Opus, Sonnet, Haiku)을 키 하나로.",
          "표준 Anthropic 동작: 스트리밍, 도구 사용, 시스템 프롬프트.",
          "키마다 선택 가능한 평생 누적 지출 한도와 만료일, 그리고 대시보드의 토큰 단위 사용량.",
        ] },
        { type: "p", text: "Cursor 사용 방식은 전혀 바뀌지 않으며, Anthropic 대신 apitoken.sale에서 키를 조달할 뿐입니다." },
      ] },
    ],
    faq: [
      { q: "이걸 하려면 Anthropic 계정이 필요한가요?", a: "아니요. apitoken.sale이 키와 잔액을 제공하므로 Anthropic 계정이 필요 없습니다." },
      { q: "이 통합은 공식 Anthropic API인가요?", a: "Cursor는 표준 Anthropic Messages API를 사용하며, apitoken.sale은 바로 그 API를 할인가로 제공합니다." },
    ],
  };
