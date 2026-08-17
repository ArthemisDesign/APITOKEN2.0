import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
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
          "Anthropic 호환 도구를 https://router.apitoken.sale로 지정하고 x-api-key 헤더와 함께 /v1/messages로 요청을 보내세요.",
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
  };
