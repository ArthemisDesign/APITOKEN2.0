import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
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
  };
