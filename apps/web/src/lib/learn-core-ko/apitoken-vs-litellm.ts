import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
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
  };
