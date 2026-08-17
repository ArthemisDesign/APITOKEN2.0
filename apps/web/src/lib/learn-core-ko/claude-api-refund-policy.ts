import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
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
  };
