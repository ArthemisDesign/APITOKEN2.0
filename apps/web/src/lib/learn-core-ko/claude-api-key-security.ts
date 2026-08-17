import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
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
  };
