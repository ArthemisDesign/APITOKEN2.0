import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "구독 없이 Claude Code 사용하기",
    h1: "월 $200 요금제 없이 쓰는 Claude Code",
    description: "월 구독 대신 종량제 API 잔액으로 Claude Code를 실행하세요. ANTHROPIC_BASE_URL을 router.apitoken.sale로 설정하고 사용한 만큼만 지불하세요.",
    keywords: ["구독 없이 claude code", "claude code api 키", "claude code 종량제", "claude code 저렴", "claude code 구독 없음"],
    dek: "Claude Code가 반드시 고정 월정액을 의미할 필요는 없습니다. 선불 잔액이 있는 API 키로 지정하면 토큰 단위로 지불하므로, 사용량이 들쭉날쭉하거나 그냥 한번 써보고 싶을 때 이상적입니다.",
    sections: [
      { h2: "환경 변수 두 개", blocks: [
        { type: "code", code: `export ANTHROPIC_BASE_URL=https://router.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••\n\n# then just run\nclaude` },
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
  };
