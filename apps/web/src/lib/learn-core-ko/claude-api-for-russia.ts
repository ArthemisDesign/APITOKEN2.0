import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
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
        { type: "p", text: "Claude Code, Cursor, Cline 또는 Anthropic SDK를 https://router.apitoken.sale로 지정하고 이전과 똑같이 작업하세요. 지원은 텔레그램에서 러시아어와 영어로 제공됩니다." },
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
  };
