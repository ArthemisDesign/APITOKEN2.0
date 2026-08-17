import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
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
  };
