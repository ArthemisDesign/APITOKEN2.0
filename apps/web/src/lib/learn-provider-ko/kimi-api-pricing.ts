import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Kimi API 가격 설명",
    h1: "Kimi API 가격: cache hit, miss, output, speed",
    description: "Kimi K3, Kimi for Coding, High Speed의 cache-hit, cache-miss, output 요금, alias mapping, apiToken.sale 고정 50% 할인을 설명합니다.",
    keywords: ["kimi api 가격", "kimi k3 가격", "kimi for coding 가격", "kimi 토큰 비용", "kimi k2.7 code 가격", "저렴한 kimi api"],
    dek: "Kimi는 cache hit, cache miss, output 요금을 따로 게시합니다. apiToken.sale는 실제 served model을 가격화하고 usage leg를 겹치지 않게 유지한 뒤 고정 50% 할인을 적용합니다.",
    sections: [
      { h2: "공개 alias의 공식 요금", blocks: [
        { type: "table", headers: ["공개 alias", "공식 hit / miss / output", "50% 할인 후"], rows: [
          ["kimi/k3 · k3-256k · k3[1m]", "$0.30 / $3 / $15", "$0.15 / $1.50 / $7.50"],
          ["kimi/kimi-for-coding", "$0.19 / $0.95 / $4", "$0.095 / $0.475 / $2"],
          ["kimi/kimi-for-coding-highspeed", "$0.38 / $1.90 / $8", "$0.19 / $0.95 / $4"],
        ] },
        { type: "p", text: "모든 값은 100만 token 기준입니다. Kimi cache는 자동이며 별도 cache-write 가격이 없으므로 새 cache token은 무료가 아니라 miss로 계산됩니다." },
      ] },
      { h2: "비용 제어 방법", blocks: [
        { type: "list", items: [
          "Kimi for Coding이 게시된 Kimi set에서 가장 저렴한 일반 coding 옵션입니다.",
          "latency가 정확히 두 배 token rate를 정당화할 때만 High Speed를 사용합니다.",
          "큰 context가 필요 없으면 full 1M 표기 대신 k3-256k를 사용합니다.",
          "key lifetime spending limit를 설정하고 대시보드에서 settled usage를 확인합니다.",
        ] },
        { type: "note", text: "reasoning token은 output의 subset이며 output rate로 결제됩니다. 별도 leg로 다시 과금되지 않습니다." },
      ] },
    ],
    faq: [
      { q: "Kimi for Coding 가격은?", a: "공식 replacement rate는 100만 cache-hit당 $0.19, cache-miss당 $0.95, output당 $4이며 apiToken.sale는 절반을 청구합니다." },
      { q: "cache hit과 miss 가격이 왜 다른가요?", a: "Kimi가 반복 context를 자동 cache합니다. terminal usage가 cache에서 제공된 input을 식별하고 각 leg가 자체 공식 rate를 사용합니다." },
      { q: "High Speed가 더 비싼가요?", a: "네. cache-hit, cache-miss, output rate가 기본 Kimi for Coding의 정확히 두 배입니다." },
    ],
  };
