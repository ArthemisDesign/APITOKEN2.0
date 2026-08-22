import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "GPT API 가격 설명",
    h1: "GPT API 가격: input, cache, output, long context",
    description: "GPT-5.6 Sol, Terra, Luna의 input, cached input, cache write, output, long-context 가격과 apiToken.sale의 고정 50% 할인을 알아보세요.",
    keywords: ["gpt api 가격", "gpt-5.6 가격", "gpt api 비용", "gpt 토큰 가격", "gpt-5.6 sol 가격", "저렴한 gpt api"],
    dek: "GPT 비용은 요청당 고정 가격이 아니라 정확한 token leg의 합입니다. 모델 tier, cached token, input 길이로 공식 비용을 계산한 다음 apiToken.sale가 50%를 할인합니다.",
    sections: [
      { h2: "현재 GPT-5.6 요금", blocks: [
        { type: "table", headers: ["모델", "공식 input / cached / cache write / output", "50% 할인 후"], rows: [
          ["gpt-5.6-sol", "$4 / $0.40 / $5 / $20", "$2 / $0.20 / $2.50 / $10"],
          ["gpt-5.6-terra", "$2 / $0.20 / $2.50 / $12", "$1 / $0.10 / $1.25 / $6"],
          ["gpt-5.6-luna", "$0.20 / $0.02 / $0.25 / $1.20", "$0.10 / $0.01 / $0.125 / $0.60"],
        ] },
        { type: "p", text: "모든 값은 100만 token 기준입니다. Sol의 임시 공식 요금은 2026-11-21까지(당일 포함, UTC) 적용되며, 2026-11-22 UTC부터 표준 input $5/output $30로 돌아갑니다. gpt-5.6은 gpt-5.6-sol의 alias이므로 별도 요금이 아니라 같은 가격을 사용합니다." },
        { type: "p", text: "Sol에서 fresh input 8,210 token, cached input 32,000 token, output 1,834 token인 호출은 임시 공식 요금으로 $0.08232이며 50% 할인 후 여기서는 $0.04116입니다." },
      ] },
      { h2: "Cache write와 long context", blocks: [
        { type: "list", items: [
          "Sol의 임시 공식 input/cached/cache write/output 요금은 $4/$0.40/$5/$20이며 50% 할인 후 $2/$0.20/$2.50/$10입니다.",
          "GPT-5.6 cache write는 일반 input의 125%, cached read는 input의 10%입니다.",
          "input이 272K token을 넘으면 전체 요청에 input 2배, output 1.5배가 적용됩니다. 프로모션 Sol에서 270K input + 2K output은 공식 $1.12이고, 273K input + 2K output은 $2.244입니다.",
          "reasoning token은 output에 포함되며 별도 leg로 중복 과금되지 않습니다. 프로모션 기간 Sol의 공식 output 요금은 100만 token당 $20입니다.",
          "대시보드는 terminal usage와 할인 후 정확한 결제액을 기록합니다.",
        ] },
        { type: "note", text: "더 저렴한 tier로 바꾸는 것이 prompt 축소보다 큰 절약이 될 수 있습니다. 임시 요금에서 Terra는 Sol input의 50%, output의 60%이며 Luna는 각각 5%, 6%입니다. 작업 난이도로 routing하세요." },
      ] },
    ],
    faq: [
      { q: "GPT-5.6은 100만 token당 얼마인가요?", a: "2026-11-21까지 Sol의 임시 공식 요금은 input $4/cached $0.40/cache write $5/output $20이며 apiToken.sale의 50% 할인 후 $2/$0.20/$2.50/$10입니다. 2026-11-22 UTC부터 표준 $5 input/$30 output 요금으로 돌아갑니다. Terra는 $2/$12, Luna는 $0.20/$1.20입니다." },
      { q: "cached input은 무엇인가요?", a: "provider가 cache에서 제공한 반복 prompt prefix입니다. 같은 token이 cached와 fresh input으로 동시에 과금되지는 않습니다." },
      { q: "long-context 가격은 언제 시작되나요?", a: "input이 272K token을 넘을 때 전체 요청에 input 2배와 output 1.5배를 적용한 뒤 할인합니다." },
    ],
  };
