import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude API 요청 한도",
    h1: "Claude API 요청 한도 이해하기",
    description: "apiToken.sale에서 429가 의미하는 것, Retry-After와 백오프로 처리하는 방법, 키 지출 가드레일과 처리량 제한의 차이.",
    keywords: ["claude api 요청 한도", "claude api 429", "anthropic rate limit", "claude api 처리량", "claude api 재시도"],
    dek: "요청 한도는 게이트웨이를 안정적으로 유지하고 잔액을 안전하게 지킵니다. 이를 잘 다루면 도구가 더 매끄럽게 돌아가고 낭비되는 지출이 없습니다.",
    sections: [
      { h2: "트래픽 제한과 지출 가드레일", blocks: [
        { type: "p", text: "apiToken.sale은 고정된 RPM 표를 공개하지 않습니다. 429는 게이트웨이 또는 업스트림 용량 제한을 뜻할 수 있습니다. 대시보드에서는 요청 처리량을 설정하지 않으며, 사용 가능한 키별 가드레일은 선택 가능한 평생 누적 지출 한도와 만료일입니다." },
      ] },
      { h2: "429 처리하기", blocks: [
        { type: "list", items: [
          "Retry-After 헤더를 존중하고 지수적으로 백오프하세요.",
          "엔드포인트를 두들기는 대신 동시성을 줄이세요.",
          "지속적으로 더 높은 처리량이 필요하면 지원팀에 문의하세요.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
    ],
    faq: [
      { q: "Claude API 요청 한도는 어떻게 되나요?", a: "apiToken.sale은 고정된 RPM 수치를 공개하지 않습니다. 429가 발생하면 Retry-After를 따르고 백오프하며 동시성을 줄이세요. 지속적으로 더 높은 처리량이 필요하면 지원팀에 문의하세요." },
      { q: "429가 뜨면 어떻게 해야 하나요?", a: "Retry-After를 존중하고 백오프하며 동시성을 줄이세요. 지속적으로 더 높은 한도가 필요하면 지원팀에 문의하세요." },
    ],
  };
