import type { LocalizedContent } from "./learn";

export const learnKo: Record<string, LocalizedContent> = {
  "how-to-buy-claude-api-key": {
    title: "Claude API 키 구매 방법",
    h1: "Claude API 키 구매 방법",
    description: "apitoken.sale에서 몇 분 만에 Claude API 키를 구매하세요. 모든 Claude 모델에 통용되는 하나의 키, 선불 잔액, 카드 또는 암호화폐 결제, Anthropic 계정 불필요.",
    keywords: ["claude api 키 구매", "claude api 구매 방법", "claude api 키", "claude api 발급", "anthropic api 키"],
    dek: "Claude를 사용하기 위해 Anthropic 계정도, 초대장도, 법인 카드도 필요 없습니다. apitoken.sale에서는 선불 잔액을 구매하고 키 하나를 발급받아, 동일한 Anthropic Messages API를 할인가로 호출하면 됩니다.",
    sections: [
      { h2: "세 단계로 키 발급받기", blocks: [
        { type: "steps", items: [
          "무료 계정을 만들고 대시보드를 여세요. 승인이나 대기열이 없습니다.",
          "API 키를 하나 생성하세요(sk-pool-… 형태). 동일한 키가 지원되는 모든 Claude 모델에서 작동합니다.",
          "Anthropic 호환 도구를 https://api.apitoken.sale로 지정하고 x-api-key 헤더와 함께 /v1/messages로 요청을 보내세요.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "결제 방식", blocks: [
        { type: "p", text: "원하는 만큼 달러 단위 정수 금액으로 충전하세요. 고정된 상품 카탈로그는 없습니다. 잔액은 선불이며 만료되지 않고, API 요청이 실행될 때만 차감됩니다." },
        { type: "list", items: [
          "안전한 결제 서비스 제공자를 통해 은행 카드 또는 암호화폐로 결제하세요.",
          "모든 요청은 공식 Anthropic API 소비로 환산된 뒤 현재 적용되는 할인이 적용됩니다.",
          "B2C 계정은 모든 요청에서 공식 소비 대비 50% 통일 할인을 받습니다.",
        ] },
      ] },
      { h2: "키로 할 수 있는 일", blocks: [
        { type: "p", text: "키 하나로 지원되는 전체 Claude 라인업(Opus, Sonnet, Haiku)을 Claude Code, Cursor, Cline, Continue, Zed 및 공식 Anthropic SDK에서 사용할 수 있습니다. 프로토콜은 전혀 바뀌지 않으며, 달라지는 것은 가격뿐입니다." },
      ] },
      { h2: "받을 수 있는 Claude 모델과 도구", blocks: [
        { type: "p", text: "Claude API 키 하나로 지원되는 전체 라인업을 하나의 잔액에서 사용할 수 있으며, 모든 Anthropic 호환 도구에서 작동합니다." },
        { type: "list", items: [
          "모델: Claude Opus 4.8 및 4.7, Sonnet 5 및 4.6, Haiku 4.5.",
          "도구: Claude Code, Cursor, Cline, Continue, Zed 및 Anthropic SDK.",
          "형식: 스트리밍과 도구 호출을 지원하는 Anthropic Messages API.",
        ] },
      ] },
    ],
    faq: [
      { q: "Claude API 키를 사려면 Anthropic 계정이 필요한가요?", a: "아니요. apitoken.sale이 자체 키와 잔액을 발급하므로 Anthropic 계정, 초대장, 승인 없이 시작할 수 있습니다." },
      { q: "키는 얼마나 빨리 활성화되나요?", a: "즉시 활성화됩니다. 대시보드에서 키를 생성하면 다음 요청부터 바로 작동하며, 대기열이나 수동 심사가 없습니다." },
      { q: "시작하는 데 비용이 얼마나 드나요?", a: "달러 단위 정수 금액으로 원하는 만큼 충전할 수 있으며, Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧도 받습니다." },
      { q: "이것이 공식 Claude API인가요?", a: "네 — 동일한 Anthropic Messages API와 동일한 Claude 모델을 제공합니다. 다른 것은 가격과 가입·결제 방식뿐입니다." },
    ],
  },
  "cheapest-claude-api": {
    title: "가장 저렴한 Claude API — 50% 통일 할인",
    h1: "Claude API를 가장 저렴하게 사용하는 방법",
    description: "Claude API 비용을 50% 절감하세요. apitoken.sale은 완전히 동일한 Anthropic Messages API를 선불 할인가로 판매합니다. 같은 모델, 같은 엔드포인트, 더 낮은 토큰당 단가.",
    keywords: ["claude api 저렴", "claude api 할인", "저렴한 claude api", "claude api 가격", "anthropic api 절약", "claude api 싸게"],
    dek: "Claude API는 토큰 단위로 과금되며, 긴 코딩 세션에서는 그 토큰이 빠르게 쌓입니다. apitoken.sale은 선불 잔액을 풀링하고 통일 할인을 적용해 동일한 API를 50% 저렴하게 제공합니다.",
    sections: [
      { h2: "왜 더 저렴한가", blocks: [
        { type: "p", text: "동일한 Anthropic Messages API에 동일한 요청을 보내고 동일한 응답을 받습니다. 내부에서 달라지는 것은 과금뿐입니다. 각 호출은 공식 요율로 측정된 뒤, 잔액에 반영되기 전에 할인이 차감됩니다." },
        { type: "list", items: [
          "B2C 계정은 공식 소비 대비 50% 통일 할인을 받습니다.",
          "모든 요청에 동일한 요율이 적용됩니다 — 해금할 것이 없습니다.",
          "B2B 물량 가격은 별도로 협의합니다.",
        ] },
      ] },
      { h2: "절감 효과가 가장 큰 곳", blocks: [
        { type: "p", text: "에이전트형 코딩, 긴 멀티턴 세션, 프롬프트 캐시를 많이 쓰는 워크플로가 토큰을 가장 많이 소모하므로 절대 절감액도 가장 큽니다. 작업마다 알맞은 모델을 고르면 효과가 한층 더 커집니다." },
        { type: "note", text: "팁: 빠르고 값싼 작업은 Haiku로 보내고, 어려운 추론에는 Opus만 사용해 잔액을 더 오래 쓰세요." },
      ] },
      { h2: "구독 없음, 락인 없음", blocks: [
        { type: "p", text: "월 요금이 없습니다. 만료되지 않는 선불 잔액을 충전하고 요청이 실행될 때만 소비하므로, 사용하지 않는 날에는 비용이 발생하지 않습니다." },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "Claude API 할인이 적용되는 방식", blocks: [
        { type: "p", text: "마진도 없고 별도의 저가 모델도 없습니다. 완전히 동일한 Claude API에 대한 할인 접근을 얻는 것입니다." },
        { type: "list", items: [
          "각 요청은 공식 Anthropic 토큰 요율로 측정됩니다.",
          "통일 50% 할인이 차감됩니다.",
          "차감 후 금액이 선불 잔액에서 빠져나갑니다.",
        ] },
        { type: "table", headers: ["모델", "공식 입력 / 출력($ / 1M)", "여기서는 (−50%)"], rows: [
          ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Opus 4.7", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Sonnet 4.6", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
        ] },
        { type: "link", text: "모델별 전체 가격(캐시 요율 포함)", href: "/models" },
        { type: "link", text: "무료 계산기로 월 비용을 추정해 보세요", href: "/tools/claude-api-cost-calculator" },
      ] },
    ],
    faq: [
      { q: "정말로 같은 Claude API인가요?", a: "네. 동일한 Anthropic Messages API, 동일한 모델 ID, 동일한 요청·응답 형식입니다. 호출당 가격만 더 낮습니다." },
      { q: "얼마나 절약할 수 있나요?", a: "B2C 가격은 모든 요청에서 공식 API 소비 대비 50% 통일 할인입니다." },
      { q: "숨은 요금이나 구독이 있나요?", a: "없습니다. 잔액은 선불이며 만료되지 않고 실제 API 사용에만 소비됩니다. 월 요금은 없습니다." },
      { q: "Anthropic에서 직접 사는 것보다 저렴한 Claude API가 있나요?", a: "네. apiToken.sale은 동일한 Anthropic API를 공식 소비 대비 50% 통일 할인으로, 구독 없이 판매합니다." },
    ],
  },
  "claude-api-for-russia": {
    title: "러시아 및 제한 지역에서의 Claude API",
    h1: "러시아에서 Claude API 사용하기",
    description: "apitoken.sale로 러시아를 비롯한 제한 지역에서 Claude API에 접근하세요. Anthropic 계정 불필요, 카드 또는 암호화폐 결제, 모든 Claude 모델에 통용되는 하나의 키.",
    keywords: ["claude api 러시아", "claude api 제한 지역", "anthropic api 러시아", "claude api 우회", "claude api 결제", "claude api vpn 없이"],
    dek: "Anthropic은 모든 국가에서 직접 판매하지 않아, 러시아 등 여러 지역의 개발자는 결제할 뚜렷한 방법이 없습니다. apitoken.sale은 그 장벽을 없앱니다. 선불 잔액을 구매하면 Anthropic의 청구 지역과 상관없이 작동하는 키를 얻습니다.",
    sections: [
      { h2: "직접 접근이 어려운 이유", blocks: [
        { type: "p", text: "Anthropic 가입에는 지원되는 청구 국가와 결제 수단이 요구되는 경우가 많습니다. 이를 충족하지 못하면 키를 받을 수 없습니다. 모델 자체는 네트워크로 접근 가능하더라도 말이죠." },
      ] },
      { h2: "apitoken.sale의 해결 방식", blocks: [
        { type: "list", items: [
          "Anthropic 계정 불필요 — 저희가 키와 잔액을 발급합니다.",
          "본인에게 맞는 방식으로 은행 카드 또는 암호화폐로 결제하세요.",
          "즉시 활성화, 대기열 없음, 법인 인증 없음.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "기존 도구와 그대로 작동", blocks: [
        { type: "p", text: "Claude Code, Cursor, Cline 또는 Anthropic SDK를 https://api.apitoken.sale로 지정하고 이전과 똑같이 작업하세요. 지원은 텔레그램에서 러시아어와 영어로 제공됩니다." },
      ] },
      { h2: "VPN 없이 러시아에서 Claude API 사용하기", blocks: [
        { type: "p", text: "키와 잔액 발급에는 Anthropic 청구 국가 요건이 없으므로 시작하는 데 외국 카드나 법인이 필요하지 않습니다. 네트워크 접근성은 본인의 연결 환경에 달려 있지만, 잔액 구매와 키 생성에는 지역 제한이 없습니다." },
      ] },
    ],
    faq: [
      { q: "러시아에서 결제할 수 있나요?", a: "네. 결제 서비스 제공자를 통해 은행 카드 또는 암호화폐로 결제할 수 있으므로, 지원되는 Anthropic 청구 국가가 필요하지 않습니다." },
      { q: "VPN이 필요한가요?", a: "Anthropic 계정이나 청구 국가는 필요하지 않습니다. 네트워크 접근성은 본인의 연결 환경에 달려 있지만, 키와 잔액 발급에는 지역 제한이 없습니다." },
      { q: "러시아어 지원이 되나요?", a: "네 — 지원은 텔레그램에서 러시아어와 영어로 제공됩니다." },
      { q: "러시아에서 Claude API 요금을 결제할 수 있나요?", a: "네 — 은행 카드 또는 암호화폐로 결제할 수 있으므로, 지원되는 Anthropic 청구 국가가 필요하지 않습니다." },
    ],
  },
  "claude-api-crypto-payment": {
    title: "암호화폐로 Claude API 결제하기",
    h1: "암호화폐로 Claude API 결제하기",
    description: "apitoken.sale에서 암호화폐 또는 은행 카드로 Claude API 잔액을 구매하세요. Anthropic 계정 불필요, 즉시 활성화, 만료되지 않는 선불 잔액.",
    keywords: ["claude api 암호화폐 결제", "암호화폐로 claude api 구매", "claude api usdt", "anthropic api 암호화폐 결제", "claude api 비트코인"],
    dek: "은행 카드를 쓸 수 없거나, 단순히 암호화폐를 선호한다면, Claude API 잔액을 암호화폐로 충전하고 즉시 시작할 수 있습니다.",
    sections: [
      { h2: "카드 또는 암호화폐, 선택은 자유", blocks: [
        { type: "p", text: "결제 시 안전한 결제 서비스 제공자를 통해 은행 카드 또는 암호화폐로 결제할 수 있습니다. 어느 쪽이든 잔액은 선불로 계정에 들어가고 요청이 실행될 때만 소비됩니다." },
      ] },
      { h2: "암호화폐가 도움이 되는 이유", blocks: [
        { type: "list", items: [
          "지원되는 Anthropic 청구 국가가 필요 없습니다.",
          "카드가 거절되거나 사용할 수 없는 곳에서 유용합니다.",
          "잔액이 만료되지 않으므로, 한 번 충전하고 개발하면서 차감해 쓰면 됩니다.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "결제 시 예상되는 것", blocks: [
        { type: "p", text: "결제에서 암호화폐를 선택하고 표시된 주소로 금액을 보내면, 네트워크가 확인하는 즉시 잔액이 반영됩니다. 특정 충전에서 카드를 선호한다면 카드도 계속 사용할 수 있습니다." },
        { type: "list", items: [
          "온체인 확인 후 잔액이 반영됩니다.",
          "달러 단위 정수 금액이면 무엇이든 가능하며, 잔액은 만료되지 않습니다.",
          "충전마다 카드와 암호화폐를 전환하세요.",
        ] },
      ] },
      { h2: "결제 가능한 암호화폐", blocks: [
        { type: "p", text: "암호화폐 충전은 안전한 결제 서비스 제공자를 통해 처리되므로 널리 쓰이는 코인이 지원됩니다." },
        { type: "list", items: [
          "USDT 및 기타 스테이블코인.",
          "BTC 및 주요 암호화폐.",
          "네트워크가 거래를 확인하면 잔액이 반영됩니다.",
        ] },
      ] },
    ],
    faq: [
      { q: "어떤 결제 수단이 지원되나요?", a: "결제 서비스 제공자를 통해 은행 카드 또는 암호화폐로 결제할 수 있습니다." },
      { q: "잔액이 만료되나요?", a: "아니요. 선불 잔액은 만료되지 않으며 실제 API 사용으로만 소비됩니다." },
      { q: "USDT로 Claude API 접근 권한을 살 수 있나요?", a: "네 — 결제 시 USDT나 기타 지원되는 암호화폐로 Claude API 잔액을 충전할 수 있습니다." },
    ],
  },
  "claude-api-without-waitlist": {
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
          "Anthropic 호환 도구를 https://api.apitoken.sale로 지정하고 x-api-key 헤더와 함께 /v1/messages로 요청을 보내세요.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "'즉시'가 실제로 의미하는 것", blocks: [
        { type: "p", text: "키를 생성하는 순간 바로 유효합니다. 가입과 첫 성공 요청 사이에 수동 심사 단계가 없으므로, 한자리에서 도구를 연결하고 출시까지 할 수 있습니다." },
      ] },
      { h2: "제로에서 첫 호출까지", blocks: [
        { type: "list", items: [
          "가입하고 대시보드를 여세요 — 승인 단계 없음.",
          "키를 생성하고 도구를 api.apitoken.sale로 지정하세요.",
          "요청을 보내고 사용량에 측정되는 것을 확인하세요.",
        ] },
        { type: "p", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로도 시작하므로, 충전 전에 전체 흐름을 검증할 수 있습니다." },
      ] },
    ],
    faq: [
      { q: "정말 대기열이 없나요?", a: "맞습니다. 접근은 셀프서비스이며 즉시입니다. 키를 생성하면 다음 요청부터 작동합니다." },
      { q: "영업팀과 이야기해야 하나요?", a: "아니요. B2C 접근은 완전히 셀프서비스입니다. 협의가 필요한 것은 협상형 B2B 물량 가격뿐입니다." },
    ],
  },
  "claude-api-quick-setup": {
    title: "2분 만에 Claude API 설정하기",
    h1: "2분 만에 Claude API 설정하기",
    description: "2분짜리 Claude API 퀵스타트: 키를 만들고, base URL을 api.apitoken.sale로 설정한 뒤, curl·Python·IDE로 첫 /v1/messages 요청을 보내세요.",
    keywords: ["claude api 퀵스타트", "claude api 설정", "claude api 첫 요청", "anthropic messages api", "claude api base url"],
    dek: "제로에서 작동하는 Claude API 호출까지 가장 빠른 길입니다. 아래 내용은 모두 표준 Anthropic Messages API를 사용하므로 기존 코드에 그대로 들어갑니다.",
    sections: [
      { h2: "1. 키 만들기", blocks: [ { type: "p", text: "가입하고 대시보드를 열어 키를 생성하세요. sk-pool-… 형태이며 지원되는 모든 모델에서 작동합니다." } ] },
      { h2: "2. 엔드포인트 설정하기", blocks: [
        { type: "p", text: "Anthropic 호환 클라이언트를 게이트웨이로 지정하세요:" },
        { type: "code", code: `Base URL:  https://api.apitoken.sale\nEndpoint:  POST /v1/messages\nHeaders:   x-api-key: sk-pool-•••\n           anthropic-version: 2023-06-01` },
      ] },
      { h2: "3. 첫 요청 보내기", blocks: [
        { type: "code", code: `curl https://api.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-opus-4-8",\n    "max_tokens": 1024,\n    "messages": [{"role":"user","content":"Hello"}]\n  }'` },
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
      { q: "어떤 base URL을 사용하나요?", a: "Anthropic 호환 도구에 https://api.apitoken.sale를 사용하고 /v1/messages로 요청을 보내세요." },
      { q: "어떤 인증 헤더가 필요한가요?", a: "공식 Anthropic API와 똑같이 키를 담은 x-api-key와 anthropic-version을 보내세요." },
    ],
  },
  "free-claude-api-key": {
    title: "시작을 위한 무료 Claude API 키",
    h1: "무료 Claude API 키로 시작하기",
    description: "Google 또는 GitHub로 apitoken.sale Claude API 키를 만들고 $5 플랫폼 웰컴 보너스 크레딧을 받으세요. 카드와 Anthropic 계정은 필요 없습니다.",
    keywords: ["무료 claude api 키", "claude api 무료", "claude api 무료 크레딧", "무료 anthropic api 키", "claude api 카드 없이"],
    dek: "Google 또는 GitHub로 계정을 만들면 $5 플랫폼 웰컴 보너스 크레딧으로 충전 전에 연동을 검증할 수 있습니다. 이메일/비밀번호 계정에는 보너스가 지급되지 않습니다.",
    sections: [
      { h2: "'무료'에 포함된 것", blocks: [
        { type: "list", items: [
          "지원되는 모든 Claude 모델에서 작동하는 API 키.",
          "Google/GitHub 신규 계정을 위한 $5 플랫폼 웰컴 보너스 크레딧.",
          "도구를 연결하고 실제 요청을 실행하기에 충분한 여유.",
        ] },
        { type: "p", text: "더 필요해지면 달러 단위 정수 금액으로 충전하세요. 할인이 자동으로 적용됩니다." },
      ] },
      { h2: "받는 방법", blocks: [
        { type: "steps", items: [
          "Google 또는 GitHub로 계정을 만들고 대시보드를 여세요. 승인이나 대기열이 없습니다.",
          "API 키를 하나 생성하세요(sk-pool-… 형태). 동일한 키가 지원되는 모든 Claude 모델에서 작동합니다.",
          "Anthropic 호환 도구를 https://api.apitoken.sale로 지정하고 x-api-key 헤더와 함께 /v1/messages로 요청을 보내세요.",
        ] },
      ] },
      { h2: "Claude API는 영원히 무료인가요?", blocks: [
        { type: "p", text: "포함된 $5 플랫폼 보너스는 무제한 무료 요금제가 아니라 무료 시작 지원금입니다. 이후에는 사용한 토큰에 대해서만 비용을 내며, 구독도 월 최소 금액도 없고 선불 잔액은 만료되지 않습니다." },
      ] },
    ],
    faq: [
      { q: "무료 사용량도 실제 API 접근인가요?", a: "네. Google/GitHub 계정의 $5 플랫폼 보너스는 유료 잔액과 동일한 지원 모델 및 엔드포인트에서 실행됩니다." },
      { q: "시작하려면 카드가 필요한가요?", a: "카드는 필요 없습니다. Google 또는 GitHub로 계정을 만들면 $5 플랫폼 웰컴 보너스 크레딧을 받습니다." },
      { q: "무료 Claude API 키에 신용카드가 필요한가요?", a: "아니요. Google 또는 GitHub로 계정을 만들면 카드 없이 $5 플랫폼 웰컴 보너스 크레딧을 받을 수 있습니다." },
    ],
  },
  "claude-api-free-trial": {
    title: "Claude API 무료 체험 — 몇 분 만에 시작",
    h1: "Claude API 무료로 사용해 보기",
    description: "몇 분 만에 Claude로 코딩을 시작하세요. Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧을 받으며 카드가 필요 없습니다.",
    keywords: ["claude api 무료 체험", "claude api 체험판", "claude api 테스트", "claude api 샌드박스", "claude api 데모"],
    dek: "별도로 신청할 체험판이 없습니다. Google 또는 GitHub로 가입하면 $5 플랫폼 웰컴 보너스 크레딧으로 지원되는 모든 모델에 실제 호출을 실행할 수 있습니다.",
    sections: [
      { h2: "결제 전에 검증하세요", blocks: [
        { type: "p", text: "포함된 사용량은 게이트웨이를 처음부터 끝까지 점검하도록 설계되었습니다. 키를 만들고, 에디터를 연결하고, 스트리밍·도구 사용·좋아하는 모델이 기대대로 작동하는지 확인하세요." },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "그다음엔 원하는 대로 확장하세요", blocks: [
        { type: "p", text: "체험 사용량이 줄어들면 원하는 금액을 충전하세요. 구독이 없고 잔액이 만료되지 않으므로, 실제로 호출한 만큼만 지불합니다." },
      ] },
    ],
    faq: [
      { q: "체험은 어떻게 시작하나요?", a: "Google 또는 GitHub로 새 계정을 만들면 $5 플랫폼 웰컴 보너스 크레딧이 자동으로 추가됩니다. 이메일/비밀번호 계정은 제외됩니다." },
      { q: "무료 사용량이 소진되면 어떻게 되나요?", a: "달러 단위 정수 금액을 충전하면 계속 사용할 수 있으며, 통일 할인이 즉시 적용됩니다." },
    ],
  },
  "claude-code-without-subscription": {
    title: "구독 없이 Claude Code 사용하기",
    h1: "월 $200 요금제 없이 쓰는 Claude Code",
    description: "월 구독 대신 종량제 API 잔액으로 Claude Code를 실행하세요. ANTHROPIC_BASE_URL을 api.apitoken.sale로 설정하고 사용한 만큼만 지불하세요.",
    keywords: ["구독 없이 claude code", "claude code api 키", "claude code 종량제", "claude code 저렴", "claude code 구독 없음"],
    dek: "Claude Code가 반드시 고정 월정액을 의미할 필요는 없습니다. 선불 잔액이 있는 API 키로 지정하면 토큰 단위로 지불하므로, 사용량이 들쭉날쭉하거나 그냥 한번 써보고 싶을 때 이상적입니다.",
    sections: [
      { h2: "환경 변수 두 개", blocks: [
        { type: "code", code: `export ANTHROPIC_BASE_URL=https://api.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••\n\n# then just run\nclaude` },
        { type: "p", text: "이게 전부입니다. Claude Code는 모든 기능을 그대로 유지하며, 구독 대신 선불 잔액에 할인가로 과금할 뿐입니다." },
      ] },
      { h2: "종량제가 유리할 때", blocks: [
        { type: "list", items: [
          "고정 월 요금이 아까운 간헐적이거나 폭발적인 사용.",
          "요금제에 약정하기 전에 Claude Code를 시험해 볼 때.",
          "여러 도구를 하나의 잔액과 하나의 키로 유지할 때.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
    ],
    faq: [
      { q: "Claude Code가 커스텀 API 키로 작동하나요?", a: "네. ANTHROPIC_BASE_URL과 ANTHROPIC_API_KEY를 설정하면 Claude Code가 여러분의 키와 잔액을 직접 사용합니다." },
      { q: "기능을 잃게 되나요?", a: "아니요. Claude Code는 동일하게 작동하며, 구독에서 선불 토큰 단위 사용으로 과금만 바뀝니다." },
    ],
  },
  "claude-opus-api": {
    title: "Claude Opus API 이용",
    h1: "API로 사용하는 Claude Opus 4.8",
    description: "하나의 apitoken.sale 키로 Claude Opus 4.8 및 4.7을 공식 요율 대비 50% 통일 할인가로 이용하세요. 복잡한 추론, 리팩터, 긴 에이전트 세션에 최적입니다.",
    keywords: ["claude opus api", "claude opus 4.8 api", "opus api 키", "claude opus 가격", "claude opus 할인"],
    dek: "Opus는 Claude의 가장 강력한 등급으로, 어려운 추론·아키텍처·긴 에이전트 실행에 손을 뻗을 모델입니다. apitoken.sale은 다른 모든 모델과 동일한 키·잔액으로 Opus 4.8과 4.7을 제공합니다.",
    sections: [
      { h2: "Opus를 언제 쓸까", blocks: [
        { type: "list", items: [
          "복잡한 리팩터와 여러 파일에 걸친 변경.",
          "아키텍처, 계획 수립, 위험이 큰 추론.",
          "일관성과 캐시 재사용이 중요한 긴 세션.",
        ] },
      ] },
      { h2: "잔액으로 쓰는 Opus", blocks: [
        { type: "p", text: "Opus 4.8(모델 ID claude-opus-4-8)과 Opus 4.7은 공식 토큰 요율에서 할인을 뺀 금액으로 과금되므로, 정가의 일부만으로 최상위 등급을 사용할 수 있습니다." },
        { type: "table", headers: ["모델", "공식 입력 / 출력($ / 1M)", "여기서는 (−50%)"], rows: [
          ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Opus 4.7", "$5 / $25", "$2.50 / $12.50"],
        ] },
        { type: "link", text: "Claude Opus 4.8 상세 가격(캐시, 컨텍스트, FAQ)", href: "/models/claude-opus-4-8" },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
    ],
    faq: [
      { q: "어떤 Opus 모델을 쓸 수 있나요?", a: "Claude Opus 4.8(claude-opus-4-8)과 Claude Opus 4.7을 Sonnet, Haiku와 동일한 키·선불 잔액으로 사용할 수 있습니다." },
      { q: "Opus는 추가 토큰만큼 가치가 있나요?", a: "복잡한 추론, 리팩터, 긴 에이전트 실행에는 그렇습니다. 빠르고 값싼 작업에는 보통 Haiku나 Sonnet이 더 나은 가성비입니다." },
    ],
  },
  "claude-sonnet-api": {
    title: "Claude Sonnet API 이용",
    h1: "API로 사용하는 Claude Sonnet",
    description: "apitoken.sale로 Claude Sonnet 5와 Sonnet 4.6을 사용하세요. 일상 코딩과 에이전트의 기본 모델을 공식 API 가격 대비 50% 통일 할인가로 이용할 수 있습니다.",
    keywords: ["claude sonnet api", "claude sonnet 5 api", "sonnet api 키", "claude sonnet 가격", "코딩에 좋은 claude 모델"],
    dek: "Sonnet은 일꾼 모델입니다. 대화형 코딩에 충분히 빠르고, 실제 에이전트 워크플로에 충분히 똑똑합니다. apitoken.sale은 하나의 할인 잔액으로 Sonnet 5와 Sonnet 4.6을 제공합니다.",
    sections: [
      { h2: "일상용 기본 모델", blocks: [
        { type: "p", text: "대부분의 코딩과 에이전트 작업에서 Sonnet이 알맞은 기본값입니다. 품질, 속도, 비용의 균형이 뛰어납니다. 진짜 어려운 문제에는 Opus를 아껴 두세요." },
      ] },
      { h2: "Sonnet 가격 참고", blocks: [
        { type: "p", text: "Claude Sonnet 5(claude-sonnet-5)는 도입 공식 요율로 제공되며, 엔진은 항상 현재 유효 요율을 적용한 뒤 할인을 반영합니다. Sonnet 4.6도 동일한 키로 계속 사용할 수 있습니다." },
        { type: "table", headers: ["모델", "공식 입력 / 출력($ / 1M)", "여기서는 (−50%)"], rows: [
          ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Sonnet 4.6", "$3 / $15", "$1.50 / $7.50"],
        ] },
        { type: "link", text: "Claude Sonnet 5 상세 가격(캐시, 컨텍스트, FAQ)", href: "/models/claude-sonnet-5" },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
    ],
    faq: [
      { q: "어떤 Sonnet 모델을 쓸 수 있나요?", a: "Claude Sonnet 5(claude-sonnet-5)와 Claude Sonnet 4.6을 Opus, Haiku와 동일한 잔액으로 사용할 수 있습니다." },
      { q: "Sonnet은 코딩에 좋나요?", a: "네 — Sonnet은 일상 코딩과 에이전트 워크플로에 권장되는 기본 모델입니다." },
    ],
  },
  "claude-haiku-api": {
    title: "Claude Haiku API 이용",
    h1: "API로 사용하는 Claude Haiku 4.5",
    description: "apitoken.sale로 Claude Haiku 4.5를 이용하세요. 가장 빠르고 가장 경제적인 Claude 모델로, 대량·저지연 작업에 이상적이며 선불 할인가로 제공됩니다.",
    keywords: ["claude haiku api", "claude haiku 4.5 api", "가장 빠른 claude 모델", "저렴한 claude 모델", "haiku api 키"],
    dek: "Haiku는 속도와 대량 처리를 위해 만들어졌습니다. 분류, 추출, 라우팅, 그리고 깊은 추론보다 지연과 비용이 더 중요한 모든 작업에 적합합니다.",
    sections: [
      { h2: "Haiku가 정답일 때", blocks: [
        { type: "list", items: [
          "대량·저지연 요청.",
          "값싼 백그라운드 작업과 전처리.",
          "Opus가 필요 없는 작업에서 잔액을 오래 쓰기.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "하나의 키로 모델 섞어 쓰기", blocks: [
        { type: "p", text: "모든 모델이 하나의 키와 잔액을 공유하므로, 값싼 작업은 Haiku(claude-haiku-4-5)로 보내고 어려운 요청만 Sonnet이나 Opus로 승격하면 됩니다." },
        { type: "table", headers: ["모델", "공식 입력 / 출력($ / 1M)", "여기서는 (−50%)"], rows: [
          ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
        ] },
        { type: "link", text: "Claude Haiku 4.5 상세 가격(캐시, 컨텍스트, FAQ)", href: "/models/claude-haiku-4-5" },
      ] },
    ],
    faq: [
      { q: "Haiku는 얼마나 빠르고 저렴한가요?", a: "Haiku 4.5는 가장 빠르고 가장 저렴한 Claude 모델로, 대량·지연 민감 작업에 이상적입니다." },
      { q: "Haiku를 다른 모델과 함께 쓸 수 있나요?", a: "네. 하나의 키와 잔액이 Haiku, Sonnet, Opus를 모두 포괄하므로 작업마다 가장 가성비 좋은 모델로 라우팅할 수 있습니다." },
    ],
  },
  "claude-api-key-for-cursor": {
    title: "Cursor용 Claude API 키",
    h1: "Cursor에서 Claude API 키 사용하기",
    description: "apitoken.sale 키로 Cursor를 Claude에 연결하세요. Anthropic base URL을 api.apitoken.sale로 설정하고 키를 붙여넣은 뒤 모델을 골라 50% 통일 할인가로 코딩하세요.",
    keywords: ["cursor claude api 키", "cursor claude api", "cursor anthropic 키", "cursor에서 claude 사용", "cursor pro 없이"],
    dek: "Cursor는 자체 Anthropic 키를 가져올 수 있게 해 주므로, 번들 플랜 대신 할인된 선불 잔액으로 Cursor에서 Claude를 실행할 수 있습니다.",
    sections: [
      { h2: "세 단계 설정", blocks: [
        { type: "steps", items: [
          "Cursor → Settings → Models → Anthropic API를 여세요.",
          "base URL을 https://api.apitoken.sale로 설정하고 sk-pool-••• 키를 붙여넣으세요.",
          "claude-opus-4-8 같은 모델을 선택하고 코딩을 시작하세요.",
        ] },
      ] },
      { h2: "구성", blocks: [
        { type: "code", code: `# Cursor → Settings → Models → Anthropic API\nBase URL : https://api.apitoken.sale\nAPI key  : sk-pool-•••\nModel    : claude-opus-4-8` },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "문제 해결", blocks: [
        { type: "list", items: [
          "Cursor가 키를 무시함: OpenAI가 아니라 Anthropic 제공자를 편집했는지 확인하세요.",
          "모델을 찾을 수 없음: claude-opus-4-8 같은 최신 모델 ID를 설정하세요.",
          "401: base URL과 키가 전부 붙여넣어졌는지 다시 확인하세요.",
        ] },
        { type: "p", text: "연결되면 지원되는 모든 Claude 모델을 동일한 키와 잔액으로 사용할 수 있습니다." },
      ] },
      { h2: "어떤 언어에서든 Cursor에서 쓰는 Claude API 키", blocks: [
        { type: "p", text: "키는 언어에 구애받지 않습니다. Cursor는 Python, JavaScript, TypeScript, Go, Rust 등 어떤 프로젝트에서든, Windows·macOS·Linux에서 이 키를 사용합니다. 설정하는 것은 모델 제공자이지 언어가 아닙니다." },
      ] },
    ],
    faq: [
      { q: "Cursor에서 내 Claude 키를 사용할 수 있나요?", a: "네. Cursor의 Anthropic 제공자는 커스텀 base URL과 키를 받아들이므로 apitoken.sale로 지정할 수 있습니다." },
      { q: "Cursor Pro가 여전히 필요한가요?", a: "자체 API 키와 잔액으로 Claude를 실행할 수 있습니다. Cursor 자체 플랜이 필요한 기능은 모델 제공자와 별개입니다." },
      { q: "Claude API 키가 Windows와 Mac의 Cursor에서 작동하나요?", a: "네 — Anthropic 제공자 설정은 Windows, macOS, Linux에서 동일합니다." },
    ],
  },
  "claude-api-for-vs-code": {
    title: "VS Code에서 Claude API 사용하기 (Cline, Continue)",
    h1: "VS Code에서 Claude API 사용하기",
    description: "apitoken.sale 키로 Cline 또는 Continue를 사용해 VS Code에서 Claude를 실행하세요. Anthropic base URL을 api.apitoken.sale로 설정하고 토큰 단위로 할인가에 결제하세요.",
    keywords: ["claude api vs code", "cline claude api", "continue claude api", "vscode claude", "vscode anthropic api 키"],
    dek: "Cline이나 Continue 같은 무료 VS Code 에이전트는 Anthropic 호환 엔드포인트를 모두 지원하므로, VS Code 안에서 할인된 잔액으로 Claude 코딩을 할 수 있습니다.",
    sections: [
      { h2: "Cline", blocks: [
        { type: "code", code: `# Cline → Settings\nAPI Provider : Anthropic\nBase URL     : https://api.apitoken.sale\nAPI Key      : sk-pool-•••\nModel        : claude-opus-4-8` },
      ] },
      { h2: "Continue", blocks: [
        { type: "code", code: `// ~/.continue/config.json\n{\n  "models": [{\n    "title": "Claude via apiToken.sale",\n    "provider": "anthropic",\n    "apiBase": "https://api.apitoken.sale",\n    "apiKey": "sk-pool-•••",\n    "model": "claude-opus-4-8"\n  }]\n}` },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "어떤 확장을 쓸지와 문제 해결", blocks: [
        { type: "p", text: "Cline은 자율 편집에 강한 기본 선택지이고, Continue는 더 가벼워서 인라인 채팅과 자동완성에 좋습니다. 둘 다 무료이며 선불 잔액을 사용합니다." },
        { type: "list", items: [
          "401 Unauthorized: API 키 또는 base URL이 잘못되었습니다.",
          "모델을 찾을 수 없음: claude-sonnet-5 또는 claude-opus-4-8 같은 최신 ID를 사용하세요.",
          "느리거나 429: 동시 요청 수를 줄이고 Retry-After를 준수하세요.",
        ] },
      ] },
    ],
    faq: [
      { q: "어떤 VS Code 확장이 작동하나요?", a: "Cline과 Continue를 포함해 Anthropic 호환 엔드포인트를 지원하는 모든 확장이 apitoken.sale 키와 함께 작동합니다." },
      { q: "유료 확장이 필요한가요?", a: "아니요. Cline과 Continue는 무료이며, 선불 잔액에서 차감되는 Claude API 사용량에 대해서만 비용을 지불합니다." },
    ],
  },
  "cursor-without-anthropic-account": {
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
        { type: "code", code: `# Cursor → Settings → Models → Anthropic API\nBase URL : https://api.apitoken.sale\nAPI key  : sk-pool-•••\nModel    : claude-opus-4-8` },
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
  },
  "anthropic-sdk-base-url": {
    title: "커스텀 Base URL로 Anthropic SDK 사용하기",
    h1: "Anthropic SDK를 apitoken.sale로 지정하기",
    description: "base_url을 api.apitoken.sale로 설정해 공식 Anthropic Python 및 TypeScript SDK를 apitoken.sale과 함께 사용하세요. 동일한 SDK, 동일한 코드, 더 낮은 토큰당 비용.",
    keywords: ["anthropic sdk base url", "anthropic python sdk 커스텀 엔드포인트", "claude sdk base url", "anthropic typescript sdk", "claude api sdk"],
    dek: "공식 Anthropic SDK는 base URL을 재정의할 수 있으므로, apitoken.sale로 전환하는 것은 한 줄만 바꾸면 됩니다. 모델 ID와 메시지 코드는 그대로 유지됩니다.",
    sections: [
      { h2: "Python", blocks: [
        { type: "code", code: `from anthropic import Anthropic\n\nclient = Anthropic(\n    base_url="https://api.apitoken.sale",\n    api_key="sk-pool-•••",\n)\nmsg = client.messages.create(\n    model="claude-opus-4-8",\n    max_tokens=1024,\n    messages=[{"role": "user", "content": "Hello"}],\n)` },
      ] },
      { h2: "TypeScript", blocks: [
        { type: "code", code: `import Anthropic from "@anthropic-ai/sdk";\n\nconst client = new Anthropic({\n  baseURL: "https://api.apitoken.sale",\n  apiKey: "sk-pool-•••",\n});\nconst msg = await client.messages.create({\n  model: "claude-opus-4-8",\n  max_tokens: 1024,\n  messages: [{ role: "user", content: "Hello" }],\n});` },
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
  },
  "apitoken-vs-anthropic-direct": {
    title: "apiToken.sale 대 Anthropic 직접 구매",
    h1: "apiToken.sale 대 Anthropic에서 직접 구매",
    description: "apiToken.sale과 Anthropic 직접 구매 비교: 동일한 Messages API와 모델이지만 50% 통일 할인, 계정 요구 없음, 카드 또는 암호화폐 결제.",
    keywords: ["claude api anthropic 직접 비교", "apitoken vs anthropic", "anthropic api 대안", "anthropic api보다 저렴", "claude api 리셀러"],
    dek: "apiToken.sale은 다른 API가 아닙니다. 선불 잔액에서 할인가로 재판매되는 동일한 Anthropic Messages API입니다. 실제로 무엇이 바뀌고 무엇이 그대로인지 살펴보겠습니다.",
    sections: [
      { h2: "그대로 유지되는 것", blocks: [
        { type: "list", items: [
          "동일한 Anthropic Messages API, 엔드포인트, 스트리밍.",
          "동일한 모델 ID(claude-opus-4-8, claude-sonnet-5, claude-haiku-4-5).",
          "여러분 코드가 이미 기대하는 동일한 요청·응답 형식.",
        ] },
      ] },
      { h2: "바뀌는 것", blocks: [
        { type: "list", items: [
          "가격: B2C의 경우 공식 소비 대비 50% 통일 할인.",
          "온보딩: Anthropic 계정, 대기열, 청구 국가 요구 없음.",
          "결제: 은행 카드 또는 암호화폐.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "각각 누구에게 맞는가", blocks: [
        { type: "p", text: "이미 마찰 없는 Anthropic 청구와 엔터프라이즈 계약이 있다면 직접 구매가 맞을 수 있습니다. 같은 모델을 더 싸게, 더 빠르게 시작하고, 카드나 암호화폐로 결제하고 싶다면 apiToken.sale이 실용적인 선택입니다." },
      ] },
    ],
    faq: [
      { q: "apiToken.sale이 진짜 Claude API인가요?", a: "네 — 동일한 Anthropic Messages API와 모델을 제공합니다. 가격과 온보딩만 다릅니다." },
      { q: "왜 Anthropic 직접 구매보다 저렴한가요?", a: "잔액이 선불로 풀링되고, 공식 소비에 50%의 통일 할인이 적용되기 때문입니다." },
    ],
  },
  "apitoken-vs-openrouter": {
    title: "Claude를 위한 apitoken.sale 대 OpenRouter",
    h1: "Claude를 위한 apitoken.sale 대 OpenRouter",
    description: "Claude 게이트웨이를 고르시나요? apitoken.sale과 OpenRouter를 비교하세요. 네이티브 Anthropic 엔드포인트와 선불 할인 대 멀티 제공자 라우터.",
    keywords: ["openrouter 대안", "apitoken 대 openrouter", "claude api 게이트웨이", "openrouter claude", "최고의 claude api 게이트웨이"],
    dek: "둘 다 Anthropic 계정 없이 Claude에 접근할 수 있게 해 주지만, 구조가 다릅니다. Claude가 주력 모델이라면 네이티브 Anthropic 엔드포인트가 일을 단순하게 유지합니다.",
    sections: [
      { h2: "네이티브 Anthropic 엔드포인트", blocks: [
        { type: "p", text: "apitoken.sale은 표준 Anthropic Messages API를 https://api.apitoken.sale에서 노출하므로, Claude Code, Cursor, Anthropic SDK가 어댑터 없이 작동합니다. 범용 멀티 제공자 추상화 계층을 거치지 않습니다." },
      ] },
      { h2: "마크업이 아닌 선불 할인", blocks: [
        { type: "list", items: [
          "공식 Claude 소비 대비 50% 통일 B2C 할인.",
          "Opus, Sonnet, Haiku를 위한 하나의 키와 잔액.",
          "만료되지 않는 카드 또는 암호화폐 충전.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "각각 언제 고를까", blocks: [
        { type: "list", items: [
          "apitoken.sale — Claude가 주력 모델이고 할인이 있는 네이티브 Anthropic 엔드포인트를 원할 때.",
          "OpenRouter — 하나의 추상화 뒤에서 여러 제공자로 라우팅해야 할 때.",
          "둘 다 Anthropic 계정 없이 시작할 수 있지만, Claude 소비를 직접 할인해 주는 것은 apitoken.sale뿐입니다.",
        ] },
      ] },
    ],
    faq: [
      { q: "왜 Claude 네이티브 게이트웨이를 골라야 하나요?", a: "Claude가 주력 모델이라면 네이티브 Anthropic 엔드포인트 덕분에 기존 Anthropic 도구와 SDK가 그대로 작동합니다." },
      { q: "apitoken.sale은 가격에 마크업을 붙이나요?", a: "아니요. 마크업을 더하는 대신 공식 Claude 소비에 할인을 적용합니다." },
    ],
  },
  "claude-opus-vs-sonnet": {
    title: "Claude Opus 대 Sonnet — 무엇을 쓸까",
    h1: "Claude Opus 대 Sonnet: 어떤 모델을 쓸까",
    description: "Opus인가 Sonnet인가? 코딩과 에이전트에 알맞은 Claude 모델을 고르는 실용 가이드 — 그리고 하나의 apiToken.sale 키·잔액으로 둘 다 사용하기.",
    keywords: ["claude opus vs sonnet", "어떤 claude 모델", "opus 아니면 sonnet 코딩", "최고의 claude 모델", "claude 모델 비교"],
    dek: "Opus와 Sonnet은 서로 다른 문제를 해결합니다. 잘 고르는 것이 더 좋은 결과를 얻고 토큰을 덜 쓰는 가장 쉬운 방법이며, 둘 다 하나의 키로 유지할 수 있습니다.",
    sections: [
      { h2: "기본값은 Sonnet으로", blocks: [
        { type: "p", text: "Sonnet 5와 Sonnet 4.6은 대다수의 코딩과 에이전트 작업을 빠르고 비용 효율적으로 처리합니다. 여기서 시작하세요." },
      ] },
      { h2: "어려운 문제는 Opus로 승격", blocks: [
        { type: "p", text: "복잡한 리팩터, 아키텍처, 추가 추론이 값을 하는 위험도 높은 긴 세션에는 Opus 4.8에 손을 뻗으세요." },
        { type: "note", text: "하나의 키가 둘 다 포괄하므로 공급자를 저글링하지 않고 작업마다 알맞은 등급으로 라우팅할 수 있습니다." },
        { type: "table", headers: ["", "Claude Opus 4.8", "Claude Sonnet 5"], rows: [
          ["공식 가격(입력 / 출력 / 1M)", "$5 / $25", "$3 / $15"],
          ["여기서는 (−50%)", "$2.50 / $12.50", "$1.50 / $7.50"],
          ["컨텍스트 윈도", "1M 토큰", "1M 토큰"],
          ["최적 용도", "어려운 추론, 긴 에이전트 실행", "일상 코딩과 에이전트"],
        ] },
        { type: "link", text: "모든 Claude 모델과 가격 비교", href: "/models" },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
    ],
    faq: [
      { q: "코딩에는 어느 쪽이 더 나은가요?", a: "일상 코딩에는 Sonnet이 권장 기본값이고, 복잡한 추론과 긴 리팩터에는 Opus를 사용하세요." },
      { q: "한 계정에서 둘 다 쓸 수 있나요?", a: "네. Opus, Sonnet, Haiku 모두 동일한 키와 선불 잔액을 공유합니다." },
    ],
  },
  "claude-api-pricing-explained": {
    title: "Claude API 가격 이해하기",
    h1: "Claude API 가격은 어떻게 작동하는가",
    description: "Claude API 가격 이해하기: 토큰당 입력·출력 요율, 프롬프트 캐싱, 그리고 apiToken.sale이 50% 통일 할인을 적용하는 방식.",
    keywords: ["claude api 가격", "claude api 비용", "claude api 가격 구조", "claude 토큰 가격", "anthropic api 가격 설명"],
    dek: "Claude는 입력과 출력에 각각 토큰 단위로 과금되며, 캐시된 콘텐츠에는 할인이 있습니다. apiToken.sale은 이 원리를 그대로 유지하고 그 위에 할인을 얹습니다.",
    sections: [
      { h2: "토큰, 입력과 출력", blocks: [
        { type: "p", text: "모든 요청은 입력 토큰(프롬프트와 컨텍스트)과 출력 토큰(모델의 응답)으로 측정됩니다. 출력 토큰은 보통 입력보다 비싸고, 큰 모델일수록 토큰당 비용이 높습니다." },
      ] },
      { h2: "캐싱과 사고(thinking)", blocks: [
        { type: "list", items: [
          "캐시 쓰기와 캐시 읽기는 별도로 측정되며, 캐시 읽기가 훨씬 저렴합니다.",
          "추론이 많은 호출에서는 사고 토큰이 출력에 산입됩니다.",
          "스트리밍과 비스트리밍 요청은 동일하게 과금됩니다.",
        ] },
      ] },
      { h2: "apiToken.sale 할인", blocks: [
        { type: "p", text: "각 호출은 공식 Anthropic 소비로 환산된 뒤 할인이 차감됩니다. B2C는 모든 요청에서 50% 통일 할인이 적용됩니다. 모든 요청은 토큰 단위 상세와 함께 대시보드에서 확인할 수 있습니다." },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "모델별 Claude API 토큰 가격", blocks: [
        { type: "p", text: "큰 모델일수록 토큰당 비용이 높습니다. Opus는 프리미엄 등급, Sonnet은 균형 잡힌 기본값, Haiku는 가장 저렴합니다. 할인은 모든 모델에 적용되므로 순위는 그대로지만 모든 가격이 낮아집니다." },
        { type: "table", headers: ["모델", "공식 입력 / 출력($ / 1M)", "여기서는 (−50%)"], rows: [
          ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Opus 4.7", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Sonnet 4.6", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
        ] },
        { type: "link", text: "캐시 요율과 컨텍스트 윈도가 있는 모델 페이지", href: "/models" },
      ] },
    ],
    faq: [
      { q: "Claude API는 어떻게 과금되나요?", a: "토큰 단위로 입력과 출력으로 나뉘며, 캐시 읽기에는 별도의 더 저렴한 요율이 적용됩니다. 큰 모델일수록 토큰당 비용이 높습니다." },
      { q: "할인은 어떻게 적용되나요?", a: "먼저 공식 소비가 계산되고, 잔액에 반영되기 전에 B2C 통일 50% 할인이 차감됩니다." },
      { q: "Claude API 토큰은 어떻게 가격이 매겨지나요?", a: "토큰 단위로 입력과 출력이 나뉘며 캐시 읽기는 더 저렴합니다. apiToken.sale은 공식 토큰 요율 위에 50% 통일 할인을 적용합니다." },
    ],
  },
  "save-tokens-on-claude-api": {
    title: "Claude API에서 토큰 절약하는 법",
    h1: "Claude API에서 토큰을 절약하는 방법",
    description: "프롬프트 캐싱, 작업별 알맞은 모델, 더 촘촘한 컨텍스트로 Claude API 비용을 줄이세요. apiToken.sale 할인과 겹쳐지는 실용적인 토큰 절약 전략입니다.",
    keywords: ["claude api 토큰 절약", "claude api 비용 절감", "claude 프롬프트 캐싱", "claude api 최적화", "claude api 요금 낮추기"],
    dek: "할인은 토큰당 가격을 낮추고, 이 전략들은 토큰 개수를 낮춥니다. 둘이 합쳐지면 청구액이 훨씬 작아집니다.",
    sections: [
      { h2: "프롬프트 캐싱 사용하기", blocks: [
        { type: "p", text: "시스템 프롬프트, 큰 파일, 도구 정의처럼 길고 안정적인 컨텍스트는 캐싱해야 합니다. 캐시 읽기는 새 입력 토큰의 일부 비용이므로 반복되는 컨텍스트가 저렴해집니다." },
      ] },
      { h2: "알맞은 모델 고르기", blocks: [
        { type: "p", text: "모든 요청을 Opus로 보내지 마세요. 값싸거나 대량인 작업은 Haiku로, 일상 코딩은 Sonnet으로, 진짜 어려운 추론에만 Opus를 아껴 두세요." },
      ] },
      { h2: "컨텍스트 다듬기", blocks: [
        { type: "list", items: [
          "작업에 실제로 필요한 파일과 이력만 보내세요.",
          "긴 스레드는 통째로 다시 보내는 대신 요약하세요.",
          "max_tokens를 응답에 실제로 필요한 만큼으로 제한하세요.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
    ],
    faq: [
      { q: "가장 큰 토큰 절약 방법 하나는?", a: "크고 반복되는 컨텍스트에 대한 프롬프트 캐싱과, 작업을 해낼 수 있는 가장 저렴한 모델 선택을 결합하는 것입니다." },
      { q: "이 팁들이 할인과 겹쳐지나요?", a: "네. 할인은 토큰당 가격을 낮추고 이 전략들은 토큰 개수를 낮추므로, 절감 효과가 곱해집니다." },
    ],
  },
  "how-billing-works": {
    title: "apiToken.sale의 과금 방식",
    h1: "과금은 어떻게 작동하는가",
    description: "apiToken.sale 과금 이해하기: 선불 잔액, 공식 요율의 요청 단위 측정, 통일 할인, 대시보드의 토큰 단위 사용량.",
    keywords: ["claude api 과금", "apitoken 과금 방식", "선불 claude api", "claude api 사용량 추적", "claude api 잔액"],
    dek: "과금은 선불이며 투명합니다. 잔액을 충전하면 각 요청이 공식 소비에서 할인을 뺀 금액을 차감하고, 감사할 수 있는 전체 내역을 제공합니다.",
    sections: [
      { h2: "선불 잔액", blocks: [
        { type: "p", text: "달러 단위 정수 금액으로 충전합니다. 잔액은 만료되지 않고 구독이 없으므로, 사용하지 않는 시간에는 비용이 들지 않습니다." },
      ] },
      { h2: "요청 단위 측정", blocks: [
        { type: "list", items: [
          "각 호출은 토큰 단위로 공식 Anthropic 소비로 환산됩니다.",
          "통일 할인(B2C의 경우 50%)이 차감됩니다.",
          "순금액이 선불 잔액에서 차감됩니다.",
        ] },
      ] },
      { h2: "완전한 가시성", blocks: [
        { type: "p", text: "모든 요청이 입력, 출력, 캐시, 사고 토큰과 함께 대시보드에 표시되므로 잔액이 어디로 가는지 항상 알 수 있습니다." },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
    ],
    faq: [
      { q: "과금은 선불인가요, 후불인가요?", a: "선불입니다. 잔액을 미리 충전하면 요청이 그것을 차감하며, 월별 청구서가 없습니다." },
      { q: "하나의 잔액으로 정말 Claude와 GPT를 모두 쓸 수 있나요?", a: "네. Claude 사용량은 Anthropic 공식 요금으로, GPT 사용량은 OpenAI 공식 요금으로 측정된 뒤 같은 할인이 적용되며, 둘 다 하나의 선불 잔액에서 차감됩니다." },
      { q: "토큰 단위 사용량을 볼 수 있나요?", a: "네. 대시보드는 사용량을 모델, 프로바이더, 토큰 버킷별로 세분화합니다." },
    ],
  },
  "claude-api-activation-time": {
    title: "Claude API 활성화는 얼마나 빠른가요?",
    h1: "Claude API 키가 얼마나 빨리 활성화되는가",
    description: "apitoken.sale 키는 즉시 활성화됩니다. 키를 생성하고 충전한 뒤 몇 분 안에 성공적인 Claude API 호출을 하세요. 수동 심사나 대기열이 없습니다.",
    keywords: ["claude api 활성화 시간", "claude api 키 얼마나 빨리", "즉시 claude api 키", "claude api 준비 시간"],
    dek: "키를 만들고 사용하기까지 대기 시간이 없습니다. 활성화는 즉시 이루어지므로, 속도의 유일한 한계는 키를 도구에 붙여넣는 속도뿐입니다.",
    sections: [
      { h2: "설계상 즉시 활성화", blocks: [
        { type: "p", text: "키는 생성하는 순간 유효합니다. 충전은 결제가 확인되는 즉시 잔액에 반영되며, 카드 결제는 몇 초 안에 확인됩니다." },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "짧은 지연을 유발할 수 있는 것", blocks: [
        { type: "p", text: "유일한 대기는 결제 확인입니다. 카드 충전은 몇 초 안에 처리되고, 암호화폐 충전은 네트워크가 거래를 확인하면 반영되며, 이는 선택한 코인과 수수료에 따라 달라집니다." },
        { type: "list", items: [
          "키 생성: 즉시.",
          "카드 충전: 수 초.",
          "암호화폐 충전: 네트워크 확인 후.",
        ] },
      ] },
    ],
    faq: [
      { q: "키가 작동하기까지 얼마나 걸리나요?", a: "즉시입니다. 수동 심사가 없으며, 새로 생성한 키는 다음 요청부터 작동합니다." },
      { q: "충전은 얼마나 걸리나요?", a: "카드 결제는 몇 초 안에 반영되고, 암호화폐는 네트워크가 거래를 확인한 뒤 반영됩니다." },
    ],
  },
  "claude-api-supported-countries": {
    title: "Claude API 지원 국가",
    h1: "apitoken.sale을 사용할 수 있는 곳",
    description: "apitoken.sale은 Anthropic 청구 국가 요건 없이 전 세계에서 작동합니다. 카드 또는 암호화폐로 결제하고, Anthropic이 직접 서비스하지 않는 지역에서도 Claude API를 사용하세요.",
    keywords: ["claude api 지원 국가", "claude api 전 세계", "anthropic api 국가 제한", "claude api 사용 가능 지역"],
    dek: "저희가 키와 잔액을 발급하므로 Anthropic 청구 국가 관문이 없습니다. 덕분에 직접 가입이 어려운 지역의 개발자도 Claude API를 사용할 수 있습니다.",
    sections: [
      { h2: "청구 국가 관문 없음", blocks: [
        { type: "list", items: [
          "Anthropic 계정이나 지원되는 청구 국가가 필요 없습니다.",
          "카드 및 암호화폐 결제 옵션.",
          "Telegram을 통한 영어 및 러시아어 지원.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "지역별 결제 방식", blocks: [
        { type: "p", text: "저희가 키와 잔액을 발급하므로 Anthropic이 지원하는 청구 국가에 묶이지 않습니다. 가능한 곳에서는 은행 카드로, 카드가 거절되는 곳에서는 암호화폐로 결제하세요." },
        { type: "list", items: [
          "Anthropic 청구 국가가 필요 없습니다.",
          "결제 시 카드 또는 암호화폐.",
          "Telegram을 통한 영어 및 러시아어 지원.",
        ] },
      ] },
    ],
    faq: [
      { q: "제 국가에서 Claude API를 사용할 수 있나요?", a: "apitoken.sale은 청구 국가 요건이 없으므로, Anthropic이 직접 청구하지 않는 지역에서도 잔액을 구매하고 키를 사용할 수 있습니다." },
      { q: "결제 제한은 어떤가요?", a: "카드 또는 암호화폐로 결제할 수 있으며, 이는 카드를 쓸 수 없는 곳에서 도움이 됩니다." },
    ],
  },
  "claude-api-refund-policy": {
    title: "Claude API 환불 정책",
    h1: "환불과 지원",
    description: "apitoken.sale이 잔액, 환불, 지원을 어떻게 처리하는지 알아보세요. 선불 잔액은 만료되지 않으며, Telegram을 통해 영어와 러시아어로 도움을 받을 수 있습니다.",
    keywords: ["claude api 환불", "apitoken 환불 정책", "claude api 지원", "claude api 환불 방법", "claude api 도움말"],
    dek: "선불 잔액은 위험이 낮도록 설계되었습니다. 만료되지 않고, 호출한 만큼만 소비되며, 지원은 메시지 한 통이면 됩니다.",
    sections: [
      { h2: "잔액과 환불", blocks: [
        { type: "p", text: "잔액은 선불이며 만료되지 않으므로, 사용하지 않은 금액은 향후 사용을 위해 그대로 남아 있습니다. 환불 처리는 원 결제 서비스 제공자를 통해 진행되며, 계정 정보와 함께 지원팀에 문의하세요." },
      ] },
      { h2: "도움 받기", blocks: [
        { type: "p", text: "지원은 Telegram을 통해 영어와 러시아어로, 그리고 apitokensale@gmail.com 이메일로 제공됩니다. 대부분의 통합 관련 질문은 빠르게 답변됩니다." },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "충전과 잔액 작동 방식", blocks: [
        { type: "p", text: "달러 단위 정수 금액으로 잔액을 추가하며, 요청이 실행될 때만 차감됩니다. 만료되지 않으므로 과도하게 충전할 이유가 거의 없습니다. 쓰는 만큼 충전하세요." },
        { type: "list", items: [
          "선불이며 만료되지 않는 잔액.",
          "원 결제 서비스 제공자를 통한 환불 처리.",
          "도움이 필요하면 계정 이메일과 함께 지원팀에 문의하세요.",
        ] },
      ] },
    ],
    faq: [
      { q: "제 잔액이 만료되나요?", a: "아니요. 선불 잔액은 만료되지 않으며 실제 API 사용으로만 소비됩니다." },
      { q: "지원팀에 어떻게 연락하나요?", a: "Telegram을 통해 영어나 러시아어로, 또는 apitokensale@gmail.com 이메일로 지원을 받으세요." },
    ],
  },
  "apitoken-vs-proxyapi": {
    title: "Claude를 위한 apitoken.sale 대 ProxyAPI",
    h1: "apitoken.sale 대 ProxyAPI",
    description: "Claude API 리셀러 비교: apitoken.sale은 50% 통일 할인, 카드 또는 암호화폐 결제, 모든 모델용 하나의 키를 갖춘 네이티브 Anthropic 엔드포인트를 제공합니다.",
    keywords: ["proxyapi 대안", "apitoken 대 proxyapi", "claude api 리셀러", "proxyapi claude", "proxyapi 없이 claude api"],
    dek: "둘 다 Anthropic 계정 없이 Claude에 접근할 수 있게 해 줍니다. 차이는 결제 방식, 절감 폭, 그리고 엔드포인트가 진정으로 Anthropic 네이티브인지에 있습니다.",
    sections: [
      { h2: "네이티브 Anthropic 엔드포인트", blocks: [
        { type: "p", text: "apitoken.sale은 표준 Anthropic Messages API를 https://api.apitoken.sale에서 노출하므로, Claude Code, Cursor, Anthropic SDK가 그대로 작동합니다. 여러분과 Claude 사이에 어댑터 계층이 없습니다." },
      ] },
      { h2: "마크업이 아닌 할인", blocks: [
        { type: "list", items: [
          "공식 Claude 소비 대비 50% 통일 B2C 할인.",
          "Opus, Sonnet, Haiku를 위한 하나의 선불 키와 잔액.",
          "만료되지 않는 카드 또는 암호화폐 충전.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "각각 언제 맞을까", blocks: [
        { type: "list", items: [
          "apitoken.sale — 통일 할인, 키의 평생 누적 지출 한도, 선택 가능한 만료일을 갖춘 네이티브 Anthropic 엔드포인트.",
          "범용 리셀러 — 이미 그곳의 다른 제공자를 쓰고 있다면 맞을 수 있습니다.",
          "둘 다 Anthropic 계정 장벽을 없애며, 차이는 가격과 Claude 접근이 얼마나 네이티브인가입니다.",
        ] },
      ] },
    ],
    faq: [
      { q: "apitoken.sale이 일반 리셀러보다 저렴한가요?", a: "정가에 마크업을 더하는 대신, 공식 Claude 소비에 50% 통일 할인을 적용합니다." },
      { q: "제 Anthropic 도구가 그대로 작동하나요?", a: "네. 네이티브 Anthropic Messages API이므로 Claude Code, Cursor, SDK는 base URL만 바꾸면 됩니다." },
    ],
  },
  "apitoken-vs-portkey": {
    title: "Claude를 위한 apiToken.sale 대 Portkey",
    h1: "apiToken.sale 대 Portkey",
    description: "Portkey는 여러분의 공급자 키를 사용해 라우팅과 관찰성을 제공하는 AI 게이트웨이입니다. apiToken.sale은 Claude 키와 잔액 자체를 할인가로 제공합니다. 각각 언제 써야 하는지 알아봅니다.",
    keywords: ["portkey 대안", "apitoken vs portkey", "ai 게이트웨이 claude", "portkey claude api", "claude api 게이트웨이"],
    dek: "이 도구들은 서로 다른 문제를 해결합니다. Portkey는 이미 보유한 공급자 키 앞에 놓이고, apiToken.sale은 Claude 키와 할인 잔액이 나오는 곳입니다.",
    sections: [
      { h2: "서로 다른 역할", blocks: [
        { type: "p", text: "Portkey는 여러분이 가져온 API 키 위에 라우팅, 캐싱, 관찰성을 더합니다. Claude 접근권이나 할인을 팔지는 않으며, 그 뒤에는 여전히 충전된 Anthropic 계정이 필요합니다." },
        { type: "p", text: "apiToken.sale은 키와 잔액의 원천입니다. https://api.apitoken.sale의 네이티브 Anthropic 엔드포인트를 50% 통일 할인으로, Anthropic 계정 없이 제공합니다." },
      ] },
      { h2: "함께 쓸 수도 있음", blocks: [
        { type: "p", text: "Portkey의 관찰성이 마음에 든다면, apiToken.sale 키를 Anthropic 공급자로 지정해 그 밑에서 할인을 받을 수 있습니다." },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
    ],
    faq: [
      { q: "Portkey가 Claude 할인을 주나요?", a: "아니요 — Portkey는 이미 보유한 키 위의 게이트웨이입니다. 할인된 Claude 키와 잔액을 제공하는 것은 apiToken.sale입니다." },
      { q: "둘을 함께 쓸 수 있나요?", a: "네. apiToken.sale 키를 Portkey의 Anthropic 공급자로 사용하면 관찰성을 유지하면서 더 적게 지불할 수 있습니다." },
    ],
  },
  "apitoken-vs-litellm": {
    title: "Claude를 위한 apitoken.sale 대 LiteLLM",
    h1: "apitoken.sale 대 LiteLLM",
    description: "LiteLLM은 모델 API를 통합하지만 자체 충전 키가 필요한 셀프호스팅 프록시입니다. apitoken.sale은 실행할 것이 없는 호스팅형 할인 Claude 엔드포인트입니다.",
    keywords: ["litellm 대안", "apitoken 대 litellm", "litellm claude", "셀프호스팅 claude 프록시", "claude api 호스팅"],
    dek: "LiteLLM은 여러 제공자에 걸친 프록시를 직접 호스팅하고 싶을 때 훌륭합니다. apitoken.sale은 그 반대의 절충안입니다. 실행할 것이 없고, Claude 잔액이 할인되어 옵니다.",
    sections: [
      { h2: "셀프호스팅 대 호스팅", blocks: [
        { type: "list", items: [
          "LiteLLM: 프록시를 직접 실행·유지해야 하고, 각 제공자에 대한 충전도 직접 합니다.",
          "apitoken.sale: 완전 호스팅형 네이티브 Anthropic 엔드포인트로, 관리할 인프라가 없습니다.",
          "apitoken.sale은 순수 프록시가 할 수 없는 50% 통일 Claude 소비 할인을 더합니다.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "각각 언제 고를까", blocks: [
        { type: "list", items: [
          "apitoken.sale — 실행할 것 없이 호스팅되는 할인 Claude 엔드포인트를 원할 때.",
          "LiteLLM — 직접 충전하는 여러 제공자에 걸친 통합 프록시를 셀프호스팅하고 싶을 때.",
          "LiteLLM을 apitoken.sale 키 앞에 두어 그 아래에서 할인을 유지할 수도 있습니다.",
        ] },
      ] },
    ],
    faq: [
      { q: "LiteLLM이 Claude를 할인해 주나요?", a: "아니요. LiteLLM은 직접 충전하는 제공자로 라우팅합니다. 할인은 apitoken.sale의 풀링된 선불 잔액에서 나옵니다." },
      { q: "apitoken.sale은 무언가 호스팅해야 하나요?", a: "아니요. 호스팅형 엔드포인트입니다. base URL과 키만 바꾸면 됩니다." },
    ],
  },
  "best-claude-model-for-coding": {
    title: "코딩에 가장 좋은 Claude 모델",
    h1: "코딩에 가장 좋은 Claude 모델",
    description: "코딩에 어떤 Claude 모델을 써야 할까요? 작업별로 Opus, Sonnet, Haiku를 고르는 실용 가이드 — 모두 하나의 apiToken.sale 키로 이용 가능합니다.",
    keywords: ["코딩에 좋은 claude 모델", "프로그래밍용 claude 모델", "opus vs sonnet 코딩", "claude 코딩 모델", "코드에 어떤 claude"],
    dek: "가장 좋은 모델은 작업에 따라 다릅니다. 모델을 작업에 맞추면 더 적은 토큰으로 더 나은 결과를 얻으며, 모든 등급이 하나의 키에 있습니다.",
    sections: [
      { h2: "일상 코딩에는 Sonnet", blocks: [
        { type: "p", text: "Claude Sonnet 5와 Sonnet 4.6은 대화형 코딩과 에이전트 루프의 기본값입니다. 빠르고, 유능하며, 비용 효율적입니다. 대부분의 작업은 여기서 시작하세요." },
      ] },
      { h2: "어려운 문제에는 Opus", blocks: [
        { type: "p", text: "복잡한 리팩터, 아키텍처, 추가 추론이 값을 하는 위험도 높은 긴 세션에는 Claude Opus 4.8을 사용하세요." },
      ] },
      { h2: "대량 처리에는 Haiku", blocks: [
        { type: "p", text: "Claude Haiku 4.5는 린팅, 추출, 빠른 편집 같은 빠르고 값싼 대량 작업을 처리해 잔액을 오래 쓰게 해줍니다." },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
    ],
    faq: [
      { q: "코딩에 가장 좋은 Claude 모델은?", a: "일상 코딩에는 Sonnet, 복잡한 추론과 리팩터에는 Opus, 빠른 대량 작업에는 Haiku입니다. 모두 하나의 apiToken.sale 키로 사용합니다." },
      { q: "요청마다 모델을 바꿀 수 있나요?", a: "네. 하나의 키와 잔액이 모든 모델을 포괄하므로 요청마다 가장 가성비 좋은 등급으로 라우팅할 수 있습니다." },
    ],
  },
  "claude-max-plan-vs-api": {
    title: "Claude Max 요금제 대 Claude API",
    h1: "Claude Max 구독 대 API",
    description: "Claude 구독을 쓸 때와 Claude API를 쓸 때. apiToken.sale은 월 요금 없이 모든 모델에 대한 종량제 API 접근을 50% 통일 할인가로 제공합니다.",
    keywords: ["claude max 요금제", "claude 구독 대 api", "claude max vs api", "claude api 종량제", "구독 없이 claude"],
    dek: "정액 Claude 구독과 종량제 API 과금은 서로 다른 사용에 맞습니다. 프로그래밍적이고 폭발적인 사용에는 보통 선불 잔액의 API가 더 나은 선택입니다.",
    sections: [
      { h2: "구독 대 토큰 단위", blocks: [
        { type: "p", text: "고정 월정액은 하나의 앱에서 꾸준하고 무거운 대화형 사용에 합리적입니다. 하지만 들쭉날쭉한 사용에는 낭비이며, 자신의 도구를 위한 프로그래밍 가능한 API 키를 주지도 않습니다." },
      ] },
      { h2: "왜 API가 더 나은 경우가 많은가", blocks: [
        { type: "list", items: [
          "실제로 사용한 토큰만큼만 지불 — 월 최저 요금 없음.",
          "하나의 키로 Claude Code, Cursor, 에이전트, 프로덕션 호출을 구동.",
          "apiToken.sale은 공식 토큰 요율에서 50%를 통일 할인.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
    ],
    faq: [
      { q: "API가 Claude 구독보다 저렴한가요?", a: "폭발적이거나 프로그래밍적인 사용에는 종량제 API 과금이 유휴 시간에 대한 정액 월 요금을 피하게 해주며, apiToken.sale이 여기에 추가 할인을 더합니다." },
      { q: "코딩 도구에서 API를 쓸 수 있나요?", a: "네 — API 키는 Claude Code, Cursor, VS Code 에이전트, SDK에서 작동하며, 이는 구독이 제공하지 않는 것입니다." },
    ],
  },
  "claude-3-5-vs-claude-4": {
    title: "Claude 3.5 대 Claude 4 — 무엇이 바뀌었나",
    h1: "Claude 3.5 대 Claude 4: 무엇이 바뀌었나",
    description: "Claude 3.5에서 현재 Claude 4 라인으로 옮기시나요? 무엇이 개선되었는지, 업데이트된 모델 ID, 그리고 apiToken.sale에서 베이스 URL 한 번 변경으로 전환하는 방법.",
    keywords: ["claude 3.5 vs 4", "claude 4 vs 3.5", "claude 모델 마이그레이션", "claude 모델 업그레이드", "새 claude 모델"],
    dek: "현재 Claude 라인은 추론과 코딩에서 3.5보다 확실히 향상되었습니다. 마이그레이션은 대부분 모델 ID 변경이며, 나머지는 그대로입니다.",
    sections: [
      { h2: "무엇이 개선되었나", blocks: [
        { type: "p", text: "Opus, Sonnet, Haiku 4 시리즈 모델은 동일한 Messages API를 유지하면서 에이전트형 코딩, 긴 컨텍스트 일관성, 복잡한 추론에서 3.5를 능가합니다." },
      ] },
      { h2: "마이그레이션 방법", blocks: [
        { type: "p", text: "모델 ID를 현재 것으로 교체하세요. 예를 들어 claude-opus-4-8, claude-sonnet-5, claude-haiku-4-5로 바꾸고 기존 요청 코드는 그대로 두면 됩니다. apiToken.sale에서는 동일한 키와 엔드포인트입니다." },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
    ],
    faq: [
      { q: "Claude 4가 3.5보다 훨씬 나은가요?", a: "네, 특히 코딩, 에이전트, 긴 컨텍스트 작업에서 그러하며, 동일한 API 형식을 사용합니다." },
      { q: "마이그레이션이 어렵나요?", a: "아니요 — 모델 ID를 업데이트하면(예: claude-sonnet-5로) 기존 Messages API 코드가 계속 작동합니다." },
    ],
  },
  "why-choose-apitoken": {
    title: "왜 apiToken.sale을 선택하는가",
    h1: "왜 apiToken.sale을 선택하는가",
    description: "개발자들이 Claude와 GPT에 apiToken.sale을 고르는 이유: 공식 Anthropic 및 OpenAI 호환 API를 50% 저렴하게, 프로바이더 계정 없이 즉시 접근, 카드 또는 암호화폐 결제.",
    keywords: ["왜 apitoken.sale", "최고의 claude api 제공자", "claude api 할인 제공자", "openai 호환 api", "anthropic 계정 없이 claude api"],
    dek: "apiToken.sale은 한 가지를 위해 만들어졌습니다. 동일한 Claude와 GPT API를 더 싸고 더 쉽게 시작하는 것. 하나의 키, 하나의 잔액, 두 개의 공식 API 서피스 — 그것이 실제로 무엇을 뜻하는지 살펴봅니다.",
    sections: [
      { h2: "요약", blocks: [
        { type: "list", items: [
          "정확히 동일한 Anthropic Messages API와 모든 현행 Claude 모델, 그리고 GPT-5 라인업을 갖춘 OpenAI 호환 API(Responses 및 Chat Completions).",
          "만료되지 않는 선불 잔액에 공식 소비 대비 50% 통일 할인 — 하나의 요율이 두 프로바이더를 모두 커버합니다.",
          "즉시, 셀프 서비스 접근 — Anthropic이나 OpenAI 계정, 대기열, 청구 국가 없음.",
          "은행 카드 또는 암호화폐로 결제.",
          "키마다 선택 가능한 평생 누적 지출 한도와 만료일, 그리고 대시보드의 토큰 단위 사용량.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "하나의 잔액으로 쓰는 할인된 API 토큰", blocks: [
        { type: "p", text: "Claude와 GPT API 토큰 할인 판매라고 생각하면 됩니다. 잔액을 한 번 선불로 충전하면 공식 토큰 요율에서 50% 통일 할인을 받고, 두 프로바이더의 모든 모델과 도구에서 사용할 수 있습니다. 잔액은 만료되지 않고 구독도 없습니다." },
      ] },
    ],
    faq: [
      { q: "apiToken.sale은 무엇이 다른가요?", a: "동일한 Claude와 GPT API를 50% 저렴하게, 프로바이더 계정 없이 즉시 접근으로, 카드나 암호화폐로 결제할 수 있는 것입니다." },
      { q: "API에 대해 바뀐 것이 있나요?", a: "아니요 — 프로토콜, 모델, 응답은 표준 Anthropic 및 OpenAI 호환 버전입니다. 가격과 온보딩만 다릅니다." },
      { q: "apiToken.sale은 무엇인가요?", a: "공식 Anthropic API와 OpenAI 호환 API에 대한 할인된 선불 접근을 판매하는 독립 멀티 프로바이더 API 게이트웨이입니다. 같은 모델을 50% 저렴하게, 프로바이더 계정 없이 이용할 수 있습니다." },
    ],
  },
  "claude-api-gateway": {
    title: "Claude API 게이트웨이란 무엇인가?",
    h1: "Claude API 게이트웨이란",
    description: "Claude API 게이트웨이는 도구와 Anthropic 사이에 위치해 접근, 과금, 제어를 더합니다. apitoken.sale은 50% 통일 할인을 제공하는 네이티브 게이트웨이입니다.",
    keywords: ["claude api 게이트웨이", "api 게이트웨이란", "anthropic 게이트웨이", "claude 프록시", "claude api 접근 계층"],
    dek: "게이트웨이는 코드와 모델 제공자 사이의 얇은 계층입니다. 좋은 Claude 게이트웨이는 도구에는 보이지 않으면서 접근, 가격, 제어를 개선합니다.",
    sections: [
      { h2: "게이트웨이가 하는 일", blocks: [
        { type: "list", items: [
          "표준 Anthropic Messages API를 제시해 도구가 그대로 작동합니다.",
          "접근과 과금을 처리합니다 — 여기서는 할인된 선불 잔액.",
          "키별 평생 누적 지출 한도, 선택 가능한 만료일, 사용량 가시성을 더합니다.",
        ] },
      ] },
      { h2: "번역 계층이 아닌 네이티브", blocks: [
        { type: "p", text: "apitoken.sale은 Anthropic 네이티브입니다. 어떤 클라이언트든 https://api.apitoken.sale/v1/messages로 지정하면 api.anthropic.com과 완전히 동일하게 동작하며, 여기에 할인과 대시보드 제어가 더해집니다." },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "게이트웨이에서 살펴봐야 할 것", blocks: [
        { type: "list", items: [
          "네이티브 Anthropic API — 도구와 SDK가 그대로 작동.",
          "대시보드에서 감사할 수 있는 투명한 토큰 단위 과금.",
          "키별 제어: 선택 가능한 평생 누적 지출 한도와 만료일.",
          "락인 없음 — 만료되지 않는 선불 잔액.",
        ] },
      ] },
    ],
    faq: [
      { q: "게이트웨이가 API를 바꾸나요?", a: "아니요. 네이티브 Claude 게이트웨이는 표준 Anthropic Messages API를 사용하므로 도구와 SDK가 그대로입니다." },
      { q: "왜 Anthropic을 직접 쓰지 않고 게이트웨이를 쓰나요?", a: "할인, Anthropic 계정 없는 즉시 접근, 개별 키의 선택 가능한 평생 누적 지출 한도와 만료일을 위해서입니다." },
    ],
  },
  "claude-api-rate-limits": {
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
  },
  "claude-api-streaming": {
    title: "Claude API로 스트리밍하기",
    h1: "Claude API에서 응답 스트리밍하기",
    description: "반응성 좋은 코딩 에이전트와 UI를 위해 apitoken.sale에서 Claude 응답을 스트리밍하는 방법. 동일한 Anthropic SSE 형식이며, 비스트리밍과 동일하게 과금됩니다.",
    keywords: ["claude api 스트리밍", "claude sse", "claude 응답 스트리밍", "anthropic 스트리밍 api", "claude api 실시간"],
    dek: "스트리밍은 토큰이 생성되는 대로 전송하므로 에이전트와 채팅 UI가 즉각적으로 느껴집니다. apitoken.sale은 표준 Anthropic 스트리밍 형식을 지원합니다.",
    sections: [
      { h2: "스트리밍하는 방법", blocks: [
        { type: "p", text: "요청에 \"stream\": true를 설정하세요(또는 SDK의 스트리밍 헬퍼를 사용하세요). 게이트웨이는 표준 Anthropic server-sent events를 반환합니다." },
        { type: "code", code: `curl https://api.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-sonnet-5",\n    "max_tokens": 1024,\n    "stream": true,\n    "messages": [{"role":"user","content":"Hello"}]\n  }'` },
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
  },
  "claude-api-prompt-caching": {
    title: "Claude API의 프롬프트 캐싱",
    h1: "Claude 프롬프트 캐싱으로 비용 줄이기",
    description: "프롬프트 캐싱은 Claude API에서 반복되는 컨텍스트를 훨씬 저렴하게 만듭니다. apiToken.sale에서 작동하는 방식, 언제 쓸지, 할인과 어떻게 겹쳐지는지 알아봅니다.",
    keywords: ["claude 프롬프트 캐싱", "claude api 캐시", "anthropic prompt cache", "캐싱으로 claude 비용 절감", "claude cache read"],
    dek: "시스템 프롬프트, 파일, 도구 정의처럼 같은 큰 컨텍스트를 반복해서 보낸다면, 캐싱이 그 토큰을 비싼 것에서 거의 공짜로 바꿔줍니다.",
    sections: [
      { h2: "캐싱이 비용을 아끼는 원리", blocks: [
        { type: "p", text: "캐시 쓰기와 캐시 읽기는 별도로 측정되며, 캐시 읽기는 새 입력 토큰의 일부 비용입니다. 안정적이고 재사용되는 컨텍스트가 이상적인 대상입니다." },
      ] },
      { h2: "할인과 겹쳐집니다", blocks: [
        { type: "p", text: "캐싱은 토큰 개수를 낮추고, apiToken.sale 할인은 토큰당 가격을 낮춥니다. 둘이 합쳐지면 청구액이 훨씬 작아지며, 모든 캐시 항목이 사용량 내역에 표시됩니다." },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
    ],
    faq: [
      { q: "프롬프트 캐싱은 얼마나 절약되나요?", a: "캐시 읽기는 새 입력 토큰의 일부 비용이므로, 반복되는 큰 컨텍스트가 훨씬 저렴해집니다." },
      { q: "캐싱이 할인과 함께 작동하나요?", a: "네 — 캐싱은 토큰 개수를 줄이고 할인은 토큰당 가격을 줄이므로 절감 효과가 곱해집니다." },
    ],
  },
  "claude-api-best-practices": {
    title: "Claude API 모범 사례",
    h1: "Claude API 모범 사례",
    description: "apitoken.sale에서의 Claude API 실전 모범 사례: 모델 선택, 프롬프트 캐싱, 스트리밍, 키의 평생 누적 지출 한도와 만료일, 안전한 키 관리.",
    keywords: ["claude api 모범 사례", "claude api 팁", "claude api 프로덕션", "claude api 가이드라인", "anthropic api 모범 사례"],
    dek: "프로덕션에서 Claude API로 안정적이고 경제적인 결과를 얻기 위한 짧은 체크리스트입니다.",
    sections: [
      { h2: "체크리스트", blocks: [
        { type: "list", items: [
          "각 작업을 처리할 수 있는 가장 값싼 모델을 고르고, 필요할 때만 상위 모델로 올리세요.",
          "크고 안정적인 컨텍스트를 캐시해 입력 비용을 크게 줄이세요.",
          "반응성 좋은 에이전트와 UI를 위해 응답을 스트리밍하세요.",
          "각 키에 선택 가능한 평생 누적 지출 한도와 만료일을 설정하세요.",
          "429는 Retry-After와 백오프로 처리하세요.",
          "토큰 단위 사용량 분석을 살펴 낭비를 일찍 잡으세요.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "비용과 안정성 관리하기", blocks: [
        { type: "list", items: [
          "max_tokens를 각 응답이 실제로 필요한 만큼으로 제한하세요.",
          "429/5xx는 촘촘한 반복 대신 지수 백오프로 재시도하세요.",
          "환경마다 이름이 분명한 별도 키를 사용해 유출 시 모든 클라이언트의 키를 바꾸지 않도록 하세요.",
          "매주 토큰 단위 사용량을 검토해 회귀를 일찍 잡으세요.",
        ] },
      ] },
    ],
    faq: [
      { q: "가장 효과적인 모범 사례는 무엇인가요?", a: "작업에 모델을 맞추고 반복되는 컨텍스트를 캐시하는 것입니다. 둘을 합치면 비용이 가장 크게 줄어듭니다." },
      { q: "키를 어떻게 안전하게 지키나요?", a: "키를 시크릿 매니저에 저장하고 적절한 평생 누적 지출 한도와 만료일을 설정하며, 노출된 키는 즉시 폐기하세요." },
    ],
  },
  "claude-code-api-key": {
    title: "API 키로 Claude Code 설정하기",
    h1: "apitoken.sale 키로 Claude Code 사용하기",
    description: "두 개의 환경 변수로 Claude Code를 apitoken.sale 키에 설정하고, 50% 통일 할인된 선불 잔액으로 모든 Claude 모델을 실행하세요.",
    keywords: ["claude code api 키", "claude code 설정", "claude code anthropic base url", "claude code 커스텀 키", "claude code 저렴하게"],
    dek: "Claude Code는 두 개의 환경 변수를 읽습니다. 이를 apitoken.sale로 지정하면 모든 기능을 유지하면서 할인된 선불 잔액으로 과금됩니다.",
    sections: [
      { h2: "두 개의 변수", blocks: [
        { type: "code", code: `export ANTHROPIC_BASE_URL=https://api.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••\n\n# then just run\nclaude` },
        { type: "p", text: "이게 설정의 전부입니다. 어려운 작업에는 claude-opus-4-8을, 일상적인 코딩에는 claude-sonnet-5를 사용하세요." },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "확인하고 모델 고르기", blocks: [
        { type: "p", text: "먼저 짧은 프롬프트를 실행해 키가 작동하는지 확인한 다음 기본 모델을 설정하세요. Claude Code가 인증 오류를 보고하면 두 환경 변수를 다시 확인하고 셸을 재시작해 변수가 export되도록 하세요." },
        { type: "list", items: [
          "일상적인 코딩: claude-sonnet-5.",
          "어려운 리팩터링과 긴 세션: claude-opus-4-8.",
          "대시보드에서 요청별 토큰 사용량을 확인해 소비를 추적하세요.",
        ] },
      ] },
    ],
    faq: [
      { q: "Claude Code를 apitoken.sale로 어떻게 지정하나요?", a: "ANTHROPIC_BASE_URL과 ANTHROPIC_API_KEY를 apitoken.sale 엔드포인트와 키로 설정한 뒤 claude를 실행하세요." },
      { q: "Claude Code의 모든 기능을 유지하나요?", a: "네. 구독에서 할인된 선불 사용량으로 과금만 바뀔 뿐입니다." },
    ],
  },
  "openai-api-quickstart": {
    title: "OpenAI 호환 API 빠른 시작 — 하나의 키로 GPT-5.6",
    h1: "OpenAI 호환 API 빠른 시작: Responses와 Chat Completions",
    description: "apiToken.sale의 OpenAI 호환 API로 GPT-5.6 모델을 실행하세요 — SSE 스트리밍을 지원하는 Responses와 Chat Completions, Claude와 공유하는 하나의 sk-pool 키와 잔액, 50% 통일 할인.",
    keywords: ["openai 호환 api", "gpt-5.6 api", "responses api", "chat completions 커스텀 base url", "openai sdk base_url", "gpt api 키", "gpt-5.6 가격"],
    dek: "sk-pool 키는 Claude 전용이 아닙니다. 같은 키와 선불 잔액으로 OpenAI 호환 엔드포인트를 통해 GPT-5 라인업을 사용할 수 있습니다 — 표준 Responses 및 Chat Completions 호출, 공식 OpenAI SDK, SSE 스트리밍, 동일한 50% 통일 할인.",
    sections: [
      { h2: "첫 GPT 호출까지 세 단계", blocks: [
        { type: "steps", items: [
          "무료 계정을 만들고 API 키 하나를 발급받으세요(sk-pool-… 형태) — 이 키는 이미 Claude 모델도 커버합니다.",
          "클라이언트를 https://openai.api.apitoken.sale/v1 로 지정하고 Authorization: Bearer로 인증하세요 — x-api-key가 아닙니다. 그 헤더는 Anthropic 서피스 전용입니다.",
          "GET /v1/models로 활성화된 모델을 확인한 뒤 Responses 요청을 보내세요.",
        ] },
        { type: "code", code: `curl https://openai.api.apitoken.sale/v1/responses \\\n  -H "Authorization: Bearer $APITOKEN_API_KEY" \\\n  -H "Content-Type: application/json" \\\n  -d '{\n    "model": "gpt-5.6-sol",\n    "input": "Reply with exactly: connected"\n  }'` },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작합니다 — Claude, GPT, Gemini 모델에 유효하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "공식 OpenAI SDK 사용", blocks: [
        { type: "p", text: "공식 SDK는 그대로 동작합니다 — base_url과 키만 바뀝니다. 프로덕션에서는 키를 서버 측 환경 변수에 보관하세요." },
        { type: "code", code: `import os\nfrom openai import OpenAI\n\nclient = OpenAI(\n    api_key=os.environ["APITOKEN_API_KEY"],\n    base_url="https://openai.api.apitoken.sale/v1",\n)\n\nresponse = client.responses.create(\n    model="gpt-5.6-sol",\n    input="Reply with exactly: connected",\n)\nprint(response.output_text)` },
        { type: "p", text: "클라이언트가 필요로 한다면 같은 호스트에서 Chat Completions도 제공됩니다 — 모델 ID와 키는 동일합니다." },
        { type: "code", code: `completion = client.chat.completions.create(\n    model="gpt-5.6-sol",\n    messages=[{"role": "user", "content": "Hello"}],\n)\nprint(completion.choices[0].message.content)` },
      ] },
      { h2: "사용 가능한 GPT 모델", blocks: [
        { type: "p", text: "제공되는 모델 세트는 엔진에 고정되어 가격이 매겨집니다. GET /v1/models가 항상 최신 답변입니다. 현재 세 가지 GPT-5.6 티어와 두 가지 이전 세대 모델을 제공합니다:" },
        { type: "table", headers: ["모델 ID", "티어", "공식 입력 / 출력($ / 1M)", "캐시 입력"], rows: [
          ["gpt-5.6-sol(별칭: gpt-5.6)", "플래그십", "$5 / $30", "$0.50"],
          ["gpt-5.6-terra", "밸런스", "$2.50 / $15", "$0.25"],
          ["gpt-5.6-luna", "고속", "$1 / $6", "$0.10"],
          ["gpt-5.5", "이전 세대 플래그십", "$5 / $30", "$0.50"],
          ["gpt-5.4", "이전 세대 밸런스", "$2.50 / $15", "$0.25"],
        ] },
        { type: "list", items: [
          "추론 강도는 요청마다 조절할 수 있습니다 — 모든 모델에서 none부터 xhigh까지, GPT-5.6 라인업은 max까지 지원합니다.",
          "모든 모델이 텍스트와 이미지 입력을 받고 Responses와 Chat Completions 모두에서 SSE로 스트리밍합니다.",
          "272K 입력 토큰을 초과하는 요청은 OpenAI 장문 컨텍스트 요금으로 청구됩니다: 전체 요청에 입력 2배, 출력 1.5배.",
          "B2C 할인은 Claude 사용량과 정확히 동일하게 적용됩니다 — 하나의 잔액, 하나의 요율, 공식 사용량 대비 50% 할인.",
        ] },
        { type: "link", text: "모델별 전체 사양과 할인 가격", href: "/models" },
      ] },
      { h2: "엔드포인트가 커버하는 범위", blocks: [
        { type: "p", text: "이것은 OpenAI Platform이 아닌 독립적인 OpenAI 호환 서비스입니다. 의도적으로 텍스트 생성만 제공합니다: 모델 목록, Responses, Chat Completions(스트리밍 및 이미지 입력 포함). 오디오, 파일, realtime, assistants, batches, fine-tuning은 제공되지 않습니다." },
        { type: "note", text: "오류는 OpenAI 봉투로 반환됩니다 — {\"error\":{\"message\",\"type\",\"param\",\"code\"}}. 401은 키 또는 인증 헤더 오류(Bearer 사용, x-api-key 아님), 402는 공유 선불 잔액 충전 필요, 404는 모델 ID 미활성화를 의미합니다 — GET /v1/models를 확인하세요." },
      ] },
    ],
    faq: [
      { q: "정말 같은 키로 Claude와 GPT를 모두 쓸 수 있나요?", a: "네. 하나의 sk-pool 키와 하나의 선불 잔액이 두 서피스를 모두 커버합니다: Claude 모델용 api.apitoken.sale의 Anthropic Messages API와 GPT 모델용 openai.api.apitoken.sale/v1의 OpenAI 호환 API. 할인도 공유됩니다." },
      { q: "OpenAI 호환 엔드포인트는 어떤 인증 헤더를 쓰나요?", a: "Authorization: Bearer sk-pool-… 입니다. x-api-key 헤더는 Anthropic 서피스 전용입니다 — OpenAI 엔드포인트에내면 401이 반환됩니다." },
      { q: "Responses와 Chat Completions 중 무엇을 쓰나요?", a: "둘 다 SSE 스트리밍으로 제공됩니다. 새 코드와 공식 SDK에는 Responses를, 클래식 형태를 기대하는 클라이언트와 프레임워크에는 Chat Completions를 사용하세요." },
      { q: "GPT 사용량은 어떻게 과금되나요?", a: "캐시 입력과 장문 컨텍스트 가격을 포함한 공식 OpenAI 요금으로 토큰당 과금된 후, 50% B2C 통일 할인이 차감되어 선불 잔액에서 청구됩니다 — Claude 사용량과 정확히 같습니다." },
    ],
  },
  "codex-cli-setup": {
    title: "apiToken.sale로 Codex CLI 설정하기 — GPT-5.6 프로필",
    h1: "apiToken.sale에서 Codex CLI 실행하기",
    description: "apiToken.sale OpenAI 호환 엔드포인트를 가리키는 이름 있는 model_providers 프로필로 Codex CLI를 설정하세요 — ChatGPT 계정 없이 선불 잔액으로 50% 통일 할인된 GPT-5.6 모델.",
    keywords: ["codex cli 설정", "codex config.toml", "codex 커스텀 모델 프로바이더", "codex api 키", "codex cli gpt-5.6", "codex responses api", "codex cli chatgpt 없이"],
    dek: "커스텀 모델 프로바이더를 지정하면 Codex CLI는 완전히 API 키 인증으로 실행됩니다. 하나의 TOML 프로필이 apiToken.sale을 가리키고, 선불 잔액이 모든 세션을 커버합니다 — ChatGPT 로그인 없이 공식 사용량 대비 50% 저렴합니다.",
    sections: [
      { h2: "프로필 만들기", blocks: [
        { type: "p", text: "다음을 ~/.codex/apitoken.config.toml로 저장하세요. 이름 있는 프로필은 기본 Codex 설정과 기존 ChatGPT 로그인을 건드리지 않습니다 — 실행할 때 명시적으로 선택합니다." },
        { type: "code", code: `# ~/.codex/apitoken.config.toml\nmodel = "gpt-5.6-sol"\nmodel_provider = "apitoken"\n\n[model_providers.apitoken]\nname = "apiToken.sale"\nbase_url = "https://openai.api.apitoken.sale/v1"\nwire_api = "responses"\nenv_key = "APITOKEN_API_KEY"` },
        { type: "p", text: "env_key는 Codex가 키를 읽는 환경 변수의 이름을 지정합니다 — 시크릿은 셸에만 있고 TOML 파일에는 절대 들어가지 않습니다." },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작합니다 — Claude, GPT, Gemini 모델에 유효하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "실행과 확인", blocks: [
        { type: "code", code: `export APITOKEN_API_KEY=sk-pool-•••\ncodex --profile apitoken` },
        { type: "list", items: [
          "항상 --profile apitoken을 명시적으로 전달해 어떤 프로바이더와 어떤 환경 변수가 활성인지 모호하지 않게 하세요.",
          "model 줄을 바꿔 프로젝트별로 모델을 전환하세요: 가장 어려운 작업은 gpt-5.6-sol, 일상적인 작업은 gpt-5.6-terra, 빠르고 저렴한 단계는 gpt-5.6-luna.",
          "같은 Bearer 키로 GET https://openai.api.apitoken.sale/v1/models를 호출하면 현재 활성화된 모델 세트를 볼 수 있습니다.",
        ] },
        { type: "note", text: "wire_api = \"responses\"가 이 게이트웨이에 맞는 값입니다 — Responses와 Chat Completions를 모두 제공하며 Codex는 Responses로 스트리밍합니다. 특정 클라이언트가 클래식 형태를 요구할 때만 \"chat\"으로 설정하세요." },
      ] },
      { h2: "만날 수 있는 오류", blocks: [
        { type: "list", items: [
          "Missing APITOKEN_API_KEY — env_key가 가리키는 변수가 codex를 실행하는 셸에 export되어 있지 않습니다. 같은 셸(또는 셸 프로필)에서 export하세요.",
          "stream error: unexpected status 401 — 키가 잘못되었거나 폐기되었거나, base_url에서 /v1 접미사가 빠졌습니다. Codex 밖에서 curl로 재현해 어느 쪽이 깨졌는지 확인하세요.",
          "stream error: unexpected status 404 — 모델 ID가 활성화되어 있지 않습니다. 추측하지 말고 GET /v1/models를 확인하세요.",
          "402 — 공유 선불 잔액을 충전해야 합니다. 기다린다고 해결되지 않습니다.",
        ] },
        { type: "link", text: "Codex 오류 전체 플레이북 — config.toml, auth.json, 스트림 오류", href: "/errors/codex" },
      ] },
    ],
    faq: [
      { q: "ChatGPT 계정이나 구독이 필요한가요?", a: "아니요. 커스텀 model_providers 프로필과 환경에 있는 프로바이더 API 키만 있으면 Codex는 완전히 API 키 인증으로 실행됩니다 — auth.json의 ChatGPT 로그인은 무관합니다." },
      { q: "기본 Codex 설정이 바뀌나요?", a: "아니요. 프로필은 자체 파일에 있으며 --profile apitoken을 전달할 때만 활성화됩니다. 기본 설정과 로그인은 그대로 유지됩니다." },
      { q: "할인이 Claude와 동일한가요?", a: "네. GPT-5.6 사용량은 공식 OpenAI 토큰 요금으로 측정되고 B2C 통일 50% 할인이 같은 선불 잔액에 적용됩니다." },
      { q: "wire_api는 Responses와 Chat Completions 중 무엇으로 하나요?", a: "wire_api = \"responses\"를 사용하세요 — 게이트웨이는 둘 다 제공하며 Codex는 Responses 스트림을 중심으로 만들어졌습니다. Chat Completions 형태는 필요한 클라이언트를 위한 것입니다." },
    ],
  },
  "vscode-ai-agents-one-prompt": {
    title: "Claude로 무료 VS Code AI 에이전트 실행하기",
    h1: "Claude로 무료 VS Code AI 에이전트 실행하기",
    description: "Cursor Pro 없이 apitoken.sale Claude 키로 Cline, Roo Code 같은 무료 VS Code 에이전트를 설정하세요. 하나의 엔드포인트, 모든 Claude 모델, 할인가.",
    keywords: ["무료 vscode ai 에이전트", "cline roo code claude", "vscode claude 에이전트", "cursor 무료 대안", "cursor 없이 claude vscode"],
    dek: "에이전트형 코딩을 하려고 Cursor Pro가 필요하지는 않습니다. 무료 VS Code 에이전트는 Anthropic 호환 키를 모두 받아들이므로, Claude가 할인된 잔액으로 VS Code에서 실행됩니다.",
    sections: [
      { h2: "에이전트를 Claude로 지정하기", blocks: [
        { type: "steps", items: [
          "Cline이나 Roo Code 같은 무료 에이전트 확장을 설치하세요.",
          "API 제공자로 Anthropic을 선택하세요.",
          "base URL을 https://api.apitoken.sale로 설정하고 sk-pool-••• 키를 붙여넣은 뒤 claude-sonnet-5 같은 모델을 선택하세요.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "작업마다 알맞은 모델 고르기", blocks: [
        { type: "list", items: [
          "claude-sonnet-5 — 일상적인 코딩과 에이전트 루프의 기본값.",
          "claude-opus-4-8 — 복잡한 리팩터링, 아키텍처, 긴 세션.",
          "claude-haiku-4-5 — 빠르고 값싼 편집과 대량 처리 단계.",
        ] },
        { type: "p", text: "키 하나로 모든 모델을 사용하므로, 계정이나 과금을 바꾸지 않고 확장에서 작업마다 모델을 전환할 수 있습니다." },
      ] },
    ],
    faq: [
      { q: "AI 코딩을 하려면 Cursor Pro가 필요한가요?", a: "아니요. Cline, Roo Code 같은 무료 VS Code 에이전트는 apitoken.sale Claude 키와 함께 작동합니다." },
      { q: "어떤 모델을 골라야 하나요?", a: "일상적인 코딩에는 claude-sonnet-5, 복잡한 작업에는 claude-opus-4-8을 사용하세요." },
    ],
  },
  "claude-api-key-security": {
    title: "Claude API 키 보안",
    h1: "Claude API 키를 안전하게 지키기",
    description: "apiToken.sale에서 Claude API 키를 보호하는 방법: 평생 누적 지출 한도, 선택 가능한 만료일, 이름이 분명한 별도 키, 즉시 폐기, 안전한 시크릿 저장.",
    keywords: ["claude api 키 보안", "api 키 보호", "claude api 키 순환", "claude api 키 관리", "anthropic 키 보안"],
    dek: "여러분의 키는 실제 잔액을 소비하므로 자격 증명처럼 다뤄야 합니다. apiToken.sale은 키가 유출되더라도 피해 범위를 제한하는 제어 수단을 제공합니다.",
    sections: [
      { h2: "위험을 제한하는 제어", blocks: [
        { type: "list", items: [
          "키에 평생 누적 지출 한도를 설정하세요.",
          "임시 접근이 자동으로 끝나야 한다면 만료일을 선택하세요.",
          "도구나 환경마다 이름이 분명한 별도 키를 발급하세요.",
          "키를 교체하려면 새 키를 만들고 클라이언트를 업데이트한 뒤 기존 키를 폐기하세요.",
        ] },
      ] },
      { h2: "기본 위생 수칙", blocks: [
        { type: "list", items: [
          "키를 git에 커밋하거나 채팅에 붙여넣지 마세요.",
          "키는 환경 변수나 시크릿 매니저에 저장하세요.",
          "키가 노출되면 즉시 폐기하고 순환하세요.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
    ],
    faq: [
      { q: "키가 유출되면 피해를 어떻게 제한하나요?", a: "평생 누적 지출 한도와 만료일을 사용하고 클라이언트별로 이름이 분명한 키를 유지하며 노출된 키를 즉시 폐기하세요." },
      { q: "키를 어디에 저장해야 하나요?", a: "환경 변수나 시크릿 매니저에 저장하세요. 절대 git에 커밋하거나 채팅에 공유하지 마세요." },
    ],
  },
  "claude-api-for-ai-agents": {
    title: "AI 에이전트를 위한 Claude API",
    h1: "AI 에이전트에 Claude API 사용하기",
    description: "apitoken.sale로 Claude API 위에 AI 에이전트를 구축하세요. 모든 모델용 하나의 키, 스트리밍, 도구 사용, 프롬프트 캐싱, 그리고 긴 실행 비용을 제어하는 키의 평생 누적 지출 한도.",
    keywords: ["claude api 에이전트", "claude ai 에이전트 api", "claude 도구 사용", "claude 에이전트 프레임워크", "claude api 자동화"],
    dek: "에이전트형 워크로드는 토큰을 많이 쓰고 오래 실행되므로 모델 선택, 캐싱, 비용 제어가 가장 중요합니다. apitoken.sale이 에이전트에 어떻게 맞는지 살펴봅니다.",
    sections: [
      { h2: "에이전트에 필요한 것", blocks: [
        { type: "list", items: [
          "스트리밍과 도구 사용 — 둘 다 Anthropic Messages API의 표준.",
          "모델 라우팅: 값싼 단계는 Haiku, 추론은 Sonnet, 가장 어려운 것은 Opus.",
          "반복되는 시스템 프롬프트와 도구 정의를 위한 프롬프트 캐싱.",
          "폭주하는 루프가 키의 한도를 넘겨 지출하지 못하도록 하는 평생 누적 지출 한도.",
        ] },
        { type: "note", text: "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작하며 이메일/비밀번호 계정은 제외됩니다." },
      ] },
      { h2: "비용을 의식한 에이전트 루프", blocks: [
        { type: "p", text: "실전 패턴: 계획과 추론은 Sonnet으로, 값싼 하위 단계와 파싱은 Haiku로 라우팅하고, 가장 어려운 호출만 Opus로 올리세요. 시스템 프롬프트와 도구 정의를 캐시해 반복되는 컨텍스트를 거의 무료로 만드세요." },
        { type: "list", items: [
          "폭주하는 루프가 한도를 넘겨 지출하지 못하도록 키의 평생 누적 지출 한도를 설정하세요.",
          "에이전트가 부분 출력에 반응할 수 있도록 스트리밍하세요.",
          "토큰 사용량을 살펴 어떤 단계가 어떤 모델을 쓸지 조정하세요.",
        ] },
      ] },
    ],
    faq: [
      { q: "Claude API가 에이전트에 좋나요?", a: "네. 스트리밍, 도구 사용, 모델 라우팅, 프롬프트 캐싱을 모두 하나의 apitoken.sale 키와 소비 제어로 제공합니다." },
      { q: "에이전트 비용을 어떻게 낮추나요?", a: "값싼 단계는 Haiku로 라우팅하고 반복되는 컨텍스트를 캐시하며 에이전트 키에 평생 누적 지출 한도를 설정하세요." },
    ],
  },
  "claude-api-langchain": {
    title: "LangChain에서 Claude API 사용하기",
    h1: "LangChain에서 Claude API 사용하기",
    description: "apitoken.sale로 LangChain을 Claude에 연결하세요. ChatAnthropic을 api.apitoken.sale로 지정하면 모델 ID는 그대로, 토큰당 비용은 50% 저렴해집니다.",
    keywords: ["claude api langchain", "langchain anthropic", "langchain claude", "chatanthropic base url", "langchain claude api 키"],
    dek: "LangChain의 Anthropic 통합은 커스텀 API URL을 지원하므로, 두 줄만 바꾸면 체인과 에이전트가 apitoken.sale을 통해 Claude로 동작합니다. 같은 모델, 더 낮은 토큰 단가입니다.",
    sections: [
      { h2: "ChatAnthropic을 게이트웨이로 지정", blocks: [
        { type: "code", code: `from langchain_anthropic import ChatAnthropic\n\nllm = ChatAnthropic(\n    model="claude-opus-4-8",\n    anthropic_api_url="https://api.apitoken.sale",\n    anthropic_api_key="sk-pool-•••",\n)\nprint(llm.invoke("Hello").content)` },
        { type: "p", text: "통합은 이것이 전부입니다. 동일한 langchain-anthropic 패키지, 동일한 모델 ID, 동일한 스트리밍과 도구 호출 — 바뀌는 것은 엔드포인트와 가격뿐입니다." },
        { type: "note", text: "Google 또는 GitHub 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작합니다. 충전 전에 도구를 연결하고 실제 호출을 실행해 보기에 충분한 금액입니다." },
      ] },
      { h2: "또는 환경 변수로 구성", blocks: [
        { type: "code", code: `export ANTHROPIC_API_URL=https://api.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••` },
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
      { q: "LangChain이 커스텀 Claude API 엔드포인트를 지원하나요?", a: "네. ChatAnthropic은 anthropic_api_url(또는 ANTHROPIC_API_URL 환경 변수)을 받으므로, https://api.apitoken.sale로 지정하고 나머지는 그대로 두면 됩니다." },
      { q: "LangChain 에이전트와 도구 호출도 작동하나요?", a: "네 — 게이트웨이는 표준 Anthropic Messages API를 제공하므로 도구 호출, 스트리밍, LangGraph 에이전트가 공식 엔드포인트와 똑같이 동작합니다." },
      { q: "LangChain에서 어떤 모델을 쓸 수 있나요?", a: "지원되는 모든 Claude 모델 — claude-opus-4-8, claude-sonnet-5, claude-haiku-4-5 등 — 을 하나의 키와 선불 잔액으로 사용할 수 있습니다." },
    ],
  },
  "claude-api-litellm": {
    title: "LiteLLM에서 Claude API 사용하기",
    h1: "LiteLLM에서 Claude API 사용하기",
    description: "apitoken.sale로 LiteLLM을 Claude에 라우팅하세요. litellm_params 또는 프록시 설정에서 api_base를 api.apitoken.sale로 지정하면 토큰당 50% 저렴합니다.",
    keywords: ["claude api litellm", "litellm anthropic", "litellm claude", "litellm api_base anthropic", "litellm 프록시 claude"],
    dek: "LiteLLM은 Anthropic을 네이티브로 지원하고 모델별 엔드포인트 재정의를 허용하므로, 설정 한 줄로 모든 Claude 트래픽이 할인 게이트웨이를 지나갑니다.",
    sections: [
      { h2: "SDK 직접 호출", blocks: [
        { type: "code", code: `import litellm\n\nresponse = litellm.completion(\n    model="anthropic/claude-opus-4-8",\n    api_base="https://api.apitoken.sale",\n    api_key="sk-pool-•••",\n    messages=[{"role": "user", "content": "Hello"}],\n)` },
        { type: "note", text: "Google 또는 GitHub 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작합니다. 충전 전에 도구를 연결하고 실제 호출을 실행해 보기에 충분한 금액입니다." },
      ] },
      { h2: "LiteLLM 프록시 설정", blocks: [
        { type: "code", code: `# config.yaml\nmodel_list:\n  - model_name: claude-opus-4-8\n    litellm_params:\n      model: anthropic/claude-opus-4-8\n      api_base: https://api.apitoken.sale\n      api_key: sk-pool-•••` },
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
      { q: "LiteLLM이 커스텀 Anthropic api_base를 지원하나요?", a: "네 — litellm.completion()이나 프록시 설정의 litellm_params에 api_base를 전달하면 LiteLLM이 Anthropic 형식 요청을 https://api.apitoken.sale로 보냅니다." },
      { q: "anthropic/ 모델 접두사는 유지하나요?", a: "네. anthropic/claude-opus-4-8(또는 지원되는 모든 모델)을 사용해 LiteLLM이 Anthropic 프로토콜을 적용하게 하세요. 바뀌는 것은 엔드포인트와 키뿐입니다." },
      { q: "LiteLLM 기반 도구에도 적용되나요?", a: "네 — LiteLLM을 거치는 모든 것(많은 코딩 에이전트 포함)이 같은 설정에서 할인 엔드포인트를 물려받습니다." },
    ],
  },
  "claude-api-aider": {
    title: "Aider에서 Claude API 사용하기",
    h1: "Aider에서 Claude API 사용하기",
    description: "apitoken.sale로 Claude에서 Aider를 실행하세요. ANTHROPIC_API_BASE와 키를 내보내고 Claude 모델을 골라 50% 통일 할인가로 터미널 페어 프로그래밍을 하세요.",
    keywords: ["claude api aider", "aider anthropic", "aider claude", "aider anthropic api base", "aider claude api 키"],
    dek: "Aider는 긴 세션에서 토큰을 빠르게 소모하는 터미널 페어 프로그래머입니다. 환경 변수 두 개로 할인 게이트웨이를 가리키게 하고 워크플로는 그대로 유지하세요.",
    sections: [
      { h2: "환경 변수 두 개", blocks: [
        { type: "code", code: `export ANTHROPIC_API_KEY=sk-pool-•••\nexport ANTHROPIC_API_BASE=https://api.apitoken.sale\n\naider --model anthropic/claude-opus-4-8` },
        { type: "p", text: "Aider는 내부적으로 LiteLLM을 통해 Anthropic 트래픽을 라우팅하며, LiteLLM은 ANTHROPIC_API_BASE를 인식합니다. 설정 파일이 필요 없습니다." },
        { type: "note", text: "Google 또는 GitHub 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작합니다. 충전 전에 도구를 연결하고 실제 호출을 실행해 보기에 충분한 금액입니다." },
      ] },
      { h2: "Aider용 모델 고르기", blocks: [
        { type: "list", items: [
          "anthropic/claude-opus-4-8 — 가장 어려운 리팩터링과 긴 에이전트 편집.",
          "anthropic/claude-sonnet-5 — 일상 기본값; Opus에 가까운 코딩 품질.",
          "anthropic/claude-haiku-4-5 — 빠른 수정과 저렴한 실험.",
        ] },
        { type: "p", text: "긴 Aider 세션이야말로 토큰 할인이 누적되는 곳입니다. 저장소 맵, diff, 다중 파일 편집이 모두 입력과 출력으로 과금됩니다." },
      ] },
    ],
    faq: [
      { q: "Aider가 커스텀 Claude 엔드포인트를 지원하나요?", a: "네. Aider는 Anthropic 모델에 LiteLLM을 사용하고, LiteLLM은 ANTHROPIC_API_BASE 환경 변수를 인식합니다. https://api.apitoken.sale로 설정하고 평소처럼 Aider를 시작하세요." },
      { q: "Aider에서 어떤 Claude 모델이 가장 좋나요?", a: "대부분의 코딩에는 claude-sonnet-5가 최선의 기본값이고, 가장 어려운 다중 파일 작업은 claude-opus-4-8로 전환하세요. 둘 다 같은 키에서 동작합니다." },
      { q: "긴 Aider 세션은 얼마나 저렴해지나요?", a: "모든 요청이 공식 토큰 요율에서 50% 통일 할인을 뺀 금액으로 과금되므로, 직접 연결로 $10짜리 세션이 여기서는 $5입니다." },
    ],
  },
  "claude-api-roo-code": {
    title: "Roo Code에서 Claude API 사용하기",
    h1: "Roo Code에서 Claude API 사용하기",
    description: "apitoken.sale로 VS Code의 Roo Code를 Claude에 연결하세요. Anthropic 제공자를 선택하고 커스텀 base URL을 켠 뒤 키를 붙여넣고 50% 통일 할인가로 코딩하세요.",
    keywords: ["claude api roo code", "roo code anthropic", "roo code claude", "roo code 커스텀 base url", "roo code api 키"],
    dek: "Roo Code는 네이티브 Anthropic 제공자와 커스텀 base URL 옵션을 갖춘 에이전트형 VS Code 확장입니다. 할인 게이트웨이 설정은 2분이면 끝납니다.",
    sections: [
      { h2: "세 단계 설정", blocks: [
        { type: "steps", items: [
          "Roo Code 설정을 열고 API 제공자로 Anthropic을 선택하세요.",
          "커스텀 base URL 옵션을 켜고 https://api.apitoken.sale로 설정한 뒤 sk-pool-… 키를 붙여넣으세요.",
          "claude-opus-4-8이나 claude-sonnet-5 같은 모델을 골라 작업을 시작하세요.",
        ] },
        { type: "note", text: "Google 또는 GitHub 신규 계정은 $5 플랫폼 웰컴 보너스 크레딧으로 시작합니다. 충전 전에 도구를 연결하고 실제 호출을 실행해 보기에 충분한 금액입니다." },
      ] },
      { h2: "Roo Code가 토큰을 태우는 이유 — 그리고 덜 내는 법", blocks: [
        { type: "p", text: "에이전트형 확장은 파일을 읽고, 계획하고, 수정하고, 재검토하는 루프를 돌기 때문에 작업 하나가 많은 모델 호출을 실행할 수 있습니다. 토큰당 할인이 가장 중요한 워크로드가 바로 이것입니다. 같은 세션이 50% 저렴하고, 대시보드에서 토큰 단위로 확인됩니다." },
        { type: "list", items: [
          "일상 작업은 claude-sonnet-5로, 어려운 작업은 claude-opus-4-8로 보내세요.",
          "프롬프트 캐싱은 더 저렴한 공식 캐시 요율로 과금되고 할인이 더해집니다.",
          "키 하나로 Roo Code, Cline, Cursor, SDK를 동시에 커버합니다.",
        ] },
      ] },
    ],
    faq: [
      { q: "Roo Code가 커스텀 Anthropic base URL을 지원하나요?", a: "네 — Anthropic 제공자 설정에 커스텀 base URL 옵션이 있습니다. https://api.apitoken.sale로 설정하고 apitoken.sale 키를 사용하세요." },
      { q: "이 키로 Roo Code에서 어떤 모델을 쓸 수 있나요?", a: "지원되는 모든 Claude 모델 — Opus 4.8과 4.7, Sonnet 5와 4.6, Haiku 4.5 — 을 하나의 키와 선불 잔액으로 사용할 수 있습니다." },
      { q: "Cline과는 무엇이 다른가요?", a: "설정은 거의 동일합니다. 둘 다 커스텀 base URL을 받는 Anthropic 제공자를 갖춘 VS Code 에이전트입니다. 선호하는 쪽을 쓰면 되고, 키는 양쪽에서 모두 작동합니다." },
    ],
  },
};
