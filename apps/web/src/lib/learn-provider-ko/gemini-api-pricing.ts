import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Gemini API 가격 설명",
    h1: "Gemini API 가격: Pro, Flash, Flash-Lite, image output",
    description: "Gemini Pro, Flash, Flash-Lite, Nano Banana 2 가격과 cached input, long context, image output, apiToken.sale 고정 50% 할인을 비교합니다.",
    keywords: ["gemini api 가격", "gemini api 비용", "gemini 토큰 가격", "gemini flash 가격", "gemini pro 가격", "저렴한 gemini api"],
    dek: "Gemini 가격은 모델 tier, cached input, output modality, Pro의 context 길이에 따라 달라집니다. gateway가 정확한 공식 leg를 결제한 뒤 50% 할인을 적용합니다.",
    sections: [
      { h2: "대표 text 모델 요금", blocks: [
        { type: "table", headers: ["모델", "공식 input / cached / output", "50% 할인 후"], rows: [
          ["gemini-3.1-pro-preview", "$2 / $0.20 / $12", "$1 / $0.10 / $6"],
          ["gemini-3.6-flash", "$1.50 / $0.15 / $7.50", "$0.75 / $0.075 / $3.75"],
          ["gemini-3.1-flash-lite", "$0.25 / $0.025 / $1.50", "$0.125 / $0.0125 / $0.75"],
          ["gemini-2.5-flash-lite", "$0.10 / $0.01 / $0.40", "$0.05 / $0.005 / $0.20"],
        ] },
        { type: "p", text: "모든 값은 100만 token 기준입니다. cached input은 provider가 보고한 독립 usage leg이며 같은 token이 fresh input에도 중복 추가되지 않습니다." },
      ] },
      { h2: "Long context와 이미지", blocks: [
        { type: "list", items: [
          "Gemini 3.1 Pro Preview는 input 200K 초과 시 전체 요청이 100만당 input $4/output $18입니다.",
          "Gemini 3.1 Flash Image text output은 $3, image output은 100만 image token당 $60입니다.",
          "Flash Image cached input은 full input rate이며 text 모델 cache 할인은 없습니다.",
          "정확한 공식 leg 계산 후 고정 50% B2C 할인이 적용됩니다.",
        ] },
      ] },
    ],
    faq: [
      { q: "가장 저렴한 Gemini 모델은?", a: "게시된 text tier 중 Gemini 2.5 Flash-Lite가 공식 input $0.10/output $0.40이며 50% 할인 후 $0.05/$0.20입니다." },
      { q: "Gemini long-context 가격은 언제 적용되나요?", a: "Gemini 3.1 Pro Preview input이 200K token을 넘으면 전체 요청에 높은 input, cached-input, output rate가 적용됩니다." },
      { q: "Gemini image output은 어떻게 과금되나요?", a: "Gemini 3.1 Flash Image는 공식적으로 100만 image-output token당 $60, 50% 할인 후 $30입니다." },
    ],
  };
