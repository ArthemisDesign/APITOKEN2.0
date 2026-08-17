import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "대기열이나 승인 없는 Claude API",
    h1: "대기열 없는 Claude API 접근",
    description: "Anthropic 대기열과 승인을 건너뛰세요. apitoken.sale에서 계정을 만들고 Claude API 키를 생성한 뒤 몇 분 안에 첫 호출을 하세요.",
    keywords: ["claude api 대기열 없음", "claude api 즉시 접근", "승인 없이 claude api", "claude api 키 빠르게", "anthropic 계정 없이 claude api"],
    dek: "승인을 기다리면 흐름이 끊깁니다. apitoken.sale은 지원되는 모든 Claude 모델에 즉시 셀프서비스로 접근할 수 있게 해 줍니다. 대기열도, 영업 통화도, 법인 인증도 없습니다.",
    sections: [
      { h2: "즉시, 셀프서비스 접근", blocks: [
        { type: "steps", items: [
          "무료 계정을 만들고 대시보드를 여세요. 승인이나 대기열이 없습니다.",
          "API 키를 하나 생성하세요(sk-pool-… 형태). 동일한 키가 지원되는 모든 Claude 모델에서 작동합니다.",
          "Anthropic 호환 도구를 https://router.apitoken.sale로 지정하고 x-api-key 헤더와 함께 /v1/messages로 요청을 보내세요.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "'즉시'가 실제로 의미하는 것", blocks: [
        { type: "p", text: "키를 생성하는 순간 바로 유효합니다. 가입과 첫 성공 요청 사이에 수동 심사 단계가 없으므로, 한자리에서 도구를 연결하고 출시까지 할 수 있습니다." },
      ] },
      { h2: "제로에서 첫 호출까지", blocks: [
        { type: "list", items: [
          "가입하고 대시보드를 여세요 — 승인 단계 없음.",
          "키를 생성하고 도구를 router.apitoken.sale로 지정하세요.",
          "요청을 보내고 사용량에 측정되는 것을 확인하세요.",
        ] },
        { type: "p", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로도 시작하므로, 충전 전에 전체 흐름을 검증할 수 있습니다." },
      ] },
    ],
    faq: [
      { q: "정말 대기열이 없나요?", a: "맞습니다. 접근은 셀프서비스이며 즉시입니다. 키를 생성하면 다음 요청부터 작동합니다." },
      { q: "영업팀과 이야기해야 하나요?", a: "아니요. B2C 접근은 완전히 셀프서비스입니다. 협의가 필요한 것은 협상형 B2B 물량 가격뿐입니다." },
    ],
  };
