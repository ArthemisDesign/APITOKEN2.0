import { learnProviderEn } from "./learn-provider-en";
import type { LearnBlock, LocalizedContent } from "./learn";

function sourceBlock(slug: string, sectionIndex: number, blockIndex: number): LearnBlock {
  const article = learnProviderEn.find((entry) => entry.slug === slug);
  if (!article) throw new Error("Unknown provider guide: " + slug);
  const block = article.sections[sectionIndex]?.blocks[blockIndex];
  if (!block) throw new Error("Missing provider guide block: " + slug + "/" + sectionIndex + "/" + blockIndex);
  return block;
}

export const learnProviderKo: Record<string, LocalizedContent> = {
  "how-to-buy-gpt-api-key": {
    title: "GPT API 키 구매 방법",
    h1: "GPT API 키 구매 방법",
    description: "선불 잔액과 카드·암호화폐 결제로 GPT API 키를 구매하고 OpenAI 호환 endpoint에서 GPT-5.6, GPT-5.5, GPT Image 2를 공식 비용의 50%로 사용하세요.",
    keywords: ["gpt api 키 구매", "gpt api 키", "openai api 키 구매", "gpt-5.6 api", "openai 호환 api", "선불 gpt api"],
    dek: "apiToken.sale 키 하나로 별도 OpenAI Platform 계정 없이 GPT 카탈로그를 사용할 수 있습니다. 잔액을 충전하고 OpenAI 호환 endpoint를 설정하면 모든 요청의 공식 비용에서 50%가 할인됩니다.",
    sections: [
      { h2: "세 단계로 GPT 키 받기", blocks: [
        { type: "steps", items: [
          "apiToken.sale 계정을 만들고 대시보드에서 키를 발급합니다.",
          "고정 상품이나 월 약정 없이 카드 또는 암호화폐로 정수 달러 금액을 충전합니다.",
          "base URL을 https://router.apitoken.sale/v1로 설정하고 Authorization: Bearer를 사용한 뒤 GET /v1/models에서 모델을 선택합니다.",
        ] },
        sourceBlock("how-to-buy-gpt-api-key", 0, 1),
      ] },
      { h2: "키에 포함되는 기능", blocks: [
        { type: "list", items: [
          "증분 SSE streaming을 지원하는 Responses와 Chat Completions.",
          "GPT-5.6 Sol, Terra, Luna, 이전 GPT tier와 별도 GPT Image 2 route.",
          "같은 키와 잔액으로 지원되는 Claude, Gemini, Kimi 모델 사용.",
          "모든 요청의 공식 provider 비용에 적용되는 고정 50% B2C 할인.",
        ] },
        { type: "note", text: "키는 서버 환경 변수에 보관하세요. GPT는 Authorization: Bearer를 사용하며 x-api-key와 x-goog-api-key는 각각 Anthropic과 Gemini 프로토콜용입니다." },
      ] },
    ],
    faq: [
      { q: "OpenAI 계정이 필요한가요?", a: "아니요. 키, 잔액, 결제는 apiToken.sale에서 관리하며 클라이언트에는 custom base URL과 Bearer 키만 필요합니다." },
      { q: "키 하나로 GPT와 Claude를 모두 쓸 수 있나요?", a: "네. 같은 sk-pool 키와 잔액이 모든 지원 provider를 포함하며 endpoint와 인증 header만 바뀝니다." },
      { q: "OpenAI Platform과 같은 서비스인가요?", a: "아니요. 자체 계정, 선불 잔액, 지원 모델 카탈로그를 가진 독립 OpenAI 호환 gateway입니다." },
    ],
  },
  "gpt-api-pricing": {
    title: "GPT API 가격 설명",
    h1: "GPT API 가격: input, cache, output, long context",
    description: "GPT-5.6 Sol, Terra, Luna의 input, cached input, cache write, output, long-context 가격과 apiToken.sale의 고정 50% 할인을 알아보세요.",
    keywords: ["gpt api 가격", "gpt-5.6 가격", "gpt api 비용", "gpt 토큰 가격", "gpt-5.6 sol 가격", "저렴한 gpt api"],
    dek: "GPT 비용은 요청당 고정 가격이 아니라 정확한 token leg의 합입니다. 모델 tier, cached token, input 길이로 공식 비용을 계산한 다음 apiToken.sale가 50%를 할인합니다.",
    sections: [
      { h2: "현재 GPT-5.6 요금", blocks: [
        { type: "table", headers: ["모델", "공식 input / cached / output", "50% 할인 후"], rows: [
          ["gpt-5.6-sol", "$5 / $0.50 / $30", "$2.50 / $0.25 / $15"],
          ["gpt-5.6-terra", "$2 / $0.20 / $12", "$1 / $0.10 / $6"],
          ["gpt-5.6-luna", "$0.20 / $0.02 / $1.20", "$0.10 / $0.01 / $0.60"],
        ] },
        { type: "p", text: "모든 값은 100만 token 기준입니다. gpt-5.6은 gpt-5.6-sol의 alias이므로 별도 요금이 아니라 같은 가격을 사용합니다." },
      ] },
      { h2: "Cache write와 long context", blocks: [
        { type: "list", items: [
          "GPT-5.6 cache write는 일반 input의 125%, cached read는 input의 10%입니다.",
          "input이 272K token을 넘으면 전체 요청에 input 2배, output 1.5배가 적용됩니다.",
          "reasoning token은 output에 포함되며 별도 leg로 중복 과금되지 않습니다.",
          "대시보드는 terminal usage와 할인 후 정확한 결제액을 기록합니다.",
        ] },
        { type: "note", text: "더 저렴한 tier로 바꾸는 것이 prompt 축소보다 큰 절약이 될 수 있습니다. Terra는 Sol의 40%, Luna는 4%이므로 작업 난이도로 routing하세요." },
      ] },
    ],
    faq: [
      { q: "GPT-5.6은 100만 token당 얼마인가요?", a: "공식적으로 Sol은 input $5/output $30, Terra는 $2/$12, Luna는 $0.20/$1.20이며 apiToken.sale가 각 leg에 50% 할인을 적용합니다." },
      { q: "cached input은 무엇인가요?", a: "provider가 cache에서 제공한 반복 prompt prefix입니다. 같은 token이 cached와 fresh input으로 동시에 과금되지는 않습니다." },
      { q: "long-context 가격은 언제 시작되나요?", a: "input이 272K token을 넘을 때 전체 요청에 input 2배와 output 1.5배를 적용한 뒤 할인합니다." },
    ],
  },
  "gpt-5-6-sol-vs-terra-vs-luna": {
    title: "GPT-5.6 Sol vs Terra vs Luna",
    h1: "GPT-5.6 Sol, Terra, Luna 비교",
    description: "GPT-5.6 Sol, Terra, Luna를 가격, reasoning effort, context, 용도별로 비교하고 coding과 production에 맞는 GPT 모델을 선택하세요.",
    keywords: ["gpt-5.6 sol vs terra", "gpt-5.6 terra vs luna", "최고의 gpt-5.6 모델", "gpt-5.6 모델", "gpt-5.6 비교", "코딩 gpt 모델"],
    dek: "GPT-5.6 제품군은 400K context, 최대 128K output, 전체 reasoning-effort 범위를 공유합니다. 실질적인 차이는 token당 구매하는 능력과 latency입니다.",
    sections: [
      { h2: "작업별 선택", blocks: [
        { type: "table", headers: ["Tier", "적합한 작업", "공식 input / output"], rows: [
          ["Sol", "어려운 reasoning, 장기 agent, 복잡한 code review", "$5 / $30"],
          ["Terra", "일상 coding, production chat, 균형 잡힌 agent", "$2 / $12"],
          ["Luna", "분류, 추출, routing, 대량 단순 작업", "$0.20 / $1.20"],
        ] },
        { type: "p", text: "Terra가 가장 안전한 기본값입니다. Sol의 controls와 context를 40% 가격에 유지합니다. eval에서 품질 차이가 확인되면 Sol로 올리고 예측 가능한 대량 작업은 Luna로 보냅니다." },
      ] },
      { h2: "공통 기능", blocks: [
        { type: "list", items: [
          "400K context와 최대 128K output.",
          "text와 image input, text output.",
          "Responses와 Chat Completions의 SSE streaming.",
          "GPT-5.6 line에서 none부터 max까지 reasoning effort.",
          "동일한 endpoint, 키, 잔액으로 작업별 모델 전환.",
        ] },
      ] },
    ],
    faq: [
      { q: "coding에 가장 좋은 GPT-5.6은 무엇인가요?", a: "일상 coding은 Terra로 시작하세요. 가장 어려운 architecture와 agent에는 Sol, 저렴한 결정적 sub-step에는 Luna가 적합합니다." },
      { q: "Sol, Terra, Luna에 서로 다른 endpoint가 필요한가요?", a: "아니요. 세 모델 모두 같은 OpenAI 호환 base URL과 키를 사용하며 model ID만 바뀝니다." },
      { q: "Terra가 max reasoning effort를 지원하나요?", a: "네. Sol, Terra, Luna 모두 max를 포함한 같은 GPT-5.6 reasoning 범위를 제공합니다." },
    ],
  },
  "gpt-image-2-api-guide": {
    title: "GPT Image 2 API 가이드",
    h1: "GPT Image 2 API로 이미지 생성 및 편집",
    description: "apiToken.sale에서 GPT Image 2를 사용하세요. 정확한 endpoint, model ID, reference image 제한, token 가격, 고정 50% 할인을 설명합니다.",
    keywords: ["gpt image 2 api", "gpt-image-2", "openai 이미지 생성 api", "gpt 이미지 편집 api", "gpt image 가격", "이미지 생성 api"],
    dek: "GPT Image 2는 별도 image route를 사용하지만 GPT text 모델과 같은 apiToken.sale 키와 잔액을 공유합니다. prompt로 생성하거나 최대 5개의 PNG reference를 편집할 수 있습니다.",
    sections: [
      { h2: "생성 route 호출", blocks: [
        sourceBlock("gpt-image-2-api-guide", 0, 0),
        { type: "p", text: "편집은 같은 모델과 최대 5개의 PNG를 multipart/form-data로 /v1/images/edits에 보냅니다. 현재 surface는 호출당 non-streaming PNG 한 장을 반환합니다." },
      ] },
      { h2: "이미지 요금 계산", blocks: [
        { type: "table", headers: ["Leg", "공식 100만 token당", "여기서의 가격"], rows: [
          ["Text input", "$5", "$2.50"],
          ["Image input", "$8", "$4"],
          ["Image output", "$30", "$15"],
        ] },
        { type: "list", items: [
          "cached text와 image input은 일반 input 가격의 25%입니다.",
          "gpt-image-2는 immutable snapshot gpt-image-2-2026-04-21의 alias입니다.",
          "image usage는 GPT, Claude, Gemini 호출과 같은 선불 잔액에서 결제됩니다.",
        ] },
      ] },
    ],
    faq: [
      { q: "GPT Image 2는 어떤 endpoint를 사용하나요?", a: "새 이미지는 POST /v1/images/generations, reference 편집은 POST /v1/images/edits를 사용합니다." },
      { q: "기존 이미지를 편집할 수 있나요?", a: "네. edits route가 multipart/form-data로 최대 5개의 PNG reference를 받습니다." },
      { q: "별도 image 키나 잔액이 필요한가요?", a: "아니요. 다른 지원 모델과 같은 Bearer 키와 선불 잔액을 사용합니다." },
    ],
  },
  "how-to-buy-gemini-api-key": {
    title: "Gemini API 키 구매 방법",
    h1: "Gemini API 키 구매 방법",
    description: "선불 잔액과 카드·암호화폐 결제로 Gemini API 키를 구매하고 native Gemini endpoint에서 Gemini, GPT, Claude, Kimi를 공식 비용의 50%로 사용하세요.",
    keywords: ["gemini api 키 구매", "gemini api 키", "google gemini api", "선불 gemini api", "gemini api 결제", "저렴한 gemini api"],
    dek: "apiToken.sale 키는 별도 Google Cloud billing 없이 native Gemini API를 제공합니다. 한 번 충전하고 x-goog-api-key로 키를 보내며 모든 지원 provider와 잔액을 공유합니다.",
    sections: [
      { h2: "세 단계로 Gemini 키 받기", blocks: [
        { type: "steps", items: [
          "apiToken.sale 계정을 만들고 대시보드에서 sk-pool 키를 발급합니다.",
          "카드 또는 암호화폐로 정수 달러 금액을 충전합니다. 잔액은 만료되지 않습니다.",
          "Gemini base URL을 https://router.apitoken.sale로 설정하고 x-goog-api-key를 사용한 뒤 GET /v1beta/models에서 모델을 선택합니다.",
        ] },
        sourceBlock("how-to-buy-gemini-api-key", 0, 1),
      ] },
      { h2: "사용 가능한 기능", blocks: [
        { type: "list", items: [
          "native Gemini protocol의 Pro, Flash, Flash-Lite text 모델.",
          "Gemini 3.1 Flash Image(Nano Banana 2) 이미지 생성.",
          "Google 형식의 generateContent, streamGenerateContent, countTokens.",
          "고정 50% B2C 할인과 GPT, Claude, Kimi가 공유하는 키/잔액.",
        ] },
        { type: "note", text: "Google SDK base URL에는 bare host만 입력하세요. SDK가 /v1beta를 추가하므로 중복 prefix는 404를 만듭니다." },
      ] },
    ],
    faq: [
      { q: "Google Cloud project가 필요한가요?", a: "아니요. gateway 계정과 billing은 apiToken.sale가 관리하며 클라이언트에는 custom base URL과 sk-pool 키만 필요합니다." },
      { q: "Gemini 인증 header는 무엇인가요?", a: "x-goog-api-key입니다. native Gemini route에서 Anthropic x-api-key나 OpenAI Authorization: Bearer를 사용하지 마세요." },
      { q: "같은 키로 GPT와 Gemini를 호출할 수 있나요?", a: "네. 키와 잔액은 공유되며 provider별 endpoint, protocol, model ID만 바뀝니다." },
    ],
  },
  "gemini-api-quickstart": {
    title: "Gemini API 빠른 시작",
    h1: "Gemini API 빠른 시작: curl과 Google GenAI SDK",
    description: "curl 또는 Google GenAI SDK로 첫 Gemini API 요청을 실행하세요. native generateContent, x-goog-api-key, 명시적 Gemini model ID를 설명합니다.",
    keywords: ["gemini api 빠른 시작", "gemini api 튜토리얼", "google genai sdk base url", "gemini generatecontent", "gemini api curl", "gemini api 예제"],
    dek: "gateway는 native Google Gemini protocol을 유지합니다. base URL과 API key만 바꾸고 generateContent와 공식 SDK 형식을 그대로 사용하되 모델을 항상 명시하세요.",
    sections: [
      { h2: "curl로 첫 요청", blocks: [
        sourceBlock("gemini-api-quickstart", 0, 0),
        { type: "p", text: "증분 출력은 streamGenerateContent?alt=sse를 사용합니다. 생성 전 무료 input 추정이 필요하면 같은 model path의 countTokens를 호출하세요." },
      ] },
      { h2: "공식 Python SDK 사용", blocks: [
        sourceBlock("gemini-api-quickstart", 1, 0),
        { type: "list", items: [
          "SDK 설정에는 /v1beta 없이 bare base URL만 전달합니다.",
          "구체적인 model ID를 지정하세요. 클라이언트의 auto default는 gateway catalog에 없을 수 있습니다.",
          "APITOKEN_API_KEY를 source code가 아닌 환경 변수에 보관합니다.",
        ] },
      ] },
    ],
    faq: [
      { q: "공식 Google GenAI SDK가 작동하나요?", a: "네. HttpOptions(base_url)을 https://router.apitoken.sale로 설정하고 apiToken.sale 키를 제공하면 request와 response 형식은 native 그대로입니다." },
      { q: "Gemini output을 streaming하려면?", a: "/v1beta/models/{model}:streamGenerateContent?alt=sse와 x-goog-api-key 또는 SDK의 대응 streaming method를 사용합니다." },
      { q: "중복 /v1beta가 왜 404를 만드나요?", a: "Google SDK가 API version을 자동으로 추가합니다. 최종 URL에 /v1beta가 한 번만 오도록 bare host만 설정하세요." },
    ],
  },
  "gemini-api-pricing": {
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
  },
  "gemini-pro-vs-flash-vs-flash-lite": {
    title: "Gemini Pro vs Flash vs Flash-Lite",
    h1: "Gemini Pro, Flash, Flash-Lite 비교",
    description: "Gemini Pro, Flash, Flash-Lite를 가격, context, reasoning, 용도별로 비교하고 coding, agent, 대량 API에 맞는 모델을 선택하세요.",
    keywords: ["gemini pro vs flash", "gemini flash vs flash lite", "최고의 gemini 모델", "gemini 모델 비교", "코딩 gemini 모델", "gemini 3.6 flash"],
    dek: "tier를 routing 결정으로 사용하세요. 가장 어려운 reasoning은 Pro, coding 기본은 Flash, 저렴한 대량 단계는 Flash-Lite가 맡습니다. 하나의 키로 모두 사용할 수 있습니다.",
    sections: [
      { h2: "작업별 선택", blocks: [
        { type: "table", headers: ["Tier", "적합한 작업", "권장 현재 ID"], rows: [
          ["Pro", "어려운 reasoning, planning, 깊은 codebase·document 분석", "gemini-3.1-pro-preview"],
          ["Flash", "일상 coding, multimodal agent, 균형 잡힌 production", "gemini-3.6-flash"],
          ["Flash-Lite", "분류, 추출, routing, 저렴한 pre-processing", "gemini-3.1-flash-lite"],
          ["Image", "이미지 생성과 편집", "gemini-3.1-flash-image"],
        ] },
        { type: "p", text: "Gemini 3.6 Flash가 대부분의 새 text workload에 좋은 시작점입니다. 가장 어려운 요청만 Pro로 올리고 예측 가능한 대량 작업은 Flash-Lite로 내립니다." },
      ] },
      { h2: "Context와 비용 trade-off", blocks: [
        { type: "list", items: [
          "현재 text 모델은 1M context와 최대 64K output을 제공합니다.",
          "Pro는 input 200K 이후 long-context premium이 있고 Flash와 Flash-Lite는 창 전체에서 flat rate입니다.",
          "text 모델 cached input은 일반적으로 fresh input의 10%입니다.",
          "큰 요청 전 countTokens를 사용하고 모델 이름보다 실제 eval로 routing하세요.",
        ] },
      ] },
    ],
    faq: [
      { q: "coding에는 어떤 Gemini가 좋은가요?", a: "Gemini 3.6 Flash로 시작하세요. 어려운 architecture와 review는 3.1 Pro Preview, 저렴한 결정적 단계는 Flash-Lite가 적합합니다." },
      { q: "Flash-Lite context가 더 작은가요?", a: "아니요. 게시된 text Flash-Lite도 1M context를 유지하며 단순 작업에서 비용과 latency가 장점입니다." },
      { q: "tier 변경에 새 키가 필요한가요?", a: "아니요. 같은 Gemini base URL과 x-goog-api-key를 유지하고 model ID만 바꾸면 됩니다." },
    ],
  },
  "nano-banana-2-api-guide": {
    title: "Nano Banana 2 API 가이드",
    h1: "Nano Banana 2 API로 이미지 생성",
    description: "native Gemini API에서 Gemini 3.1 Flash Image(Nano Banana 2)를 사용하세요. 정확한 model ID, generateContent, image-output 가격과 50% 할인을 설명합니다.",
    keywords: ["nano banana 2 api", "gemini 3.1 flash image api", "gemini 이미지 생성 api", "nano banana api 키", "gemini image 가격", "google image api"],
    dek: "Nano Banana 2는 Gemini 3.1 Flash Image의 공개 이름입니다. native generateContent를 사용하고 multimodal input을 받으며 text 모델과 같은 잔액에서 렌더링된 이미지를 반환합니다.",
    sections: [
      { h2: "정확한 model ID 사용", blocks: [
        sourceBlock("nano-banana-2-api-guide", 0, 0),
        { type: "p", text: "response part를 MIME type으로 나누세요. text part는 설명, image part는 렌더링된 asset입니다. marketing 이름 대신 gemini-3.1-flash-image를 사용합니다." },
      ] },
      { h2: "제한과 가격", blocks: [
        { type: "list", items: [
          "128K context와 최대 32K output으로 text Flash line보다 작습니다.",
          "공식 text input/output은 100만당 $0.50/$3, image output은 $60입니다.",
          "apiToken.sale 50% 할인 후 $0.25/$1.50, image output $30입니다.",
          "이 image 모델의 cached input은 full $0.50 input rate를 유지합니다.",
        ] },
        { type: "note", text: "text만 필요하면 text Flash를 사용하세요. response에 렌더링 이미지가 필요할 때 Flash Image를 사용하며 image-output leg가 별도 과금됩니다." },
      ] },
    ],
    faq: [
      { q: "Nano Banana 2 API model ID는?", a: "native Gemini generateContent route의 gemini-3.1-flash-image입니다." },
      { q: "Nano Banana 2 image output 가격은?", a: "공식 100만 image-output token당 $60, apiToken.sale 50% 할인 후 $30입니다." },
      { q: "별도 image API 키가 필요한가요?", a: "아니요. x-goog-api-key에 같은 sk-pool 키를 사용하고 선불 잔액을 공유합니다." },
    ],
  },
  "how-to-buy-kimi-api-key": {
    title: "Kimi API 키 구매 방법",
    h1: "Kimi API 키 구매 방법",
    description: "Kimi K3와 Kimi for Coding용 선불 API 키를 구매해 Anthropic Messages 또는 OpenAI 호환 client에서 사용하고 공식 API 비용의 50%로 이용하세요.",
    keywords: ["kimi api 키 구매", "kimi api 키", "kimi k3 api", "kimi for coding api", "moonshot kimi api", "선불 kimi api"],
    dek: "Kimi는 unified router의 독립 모델 namespace로 제공됩니다. native Anthropic Messages route 또는 OpenAI 호환 client를 사용하며 Claude, GPT, Gemini와 같은 선불 잔액을 공유합니다.",
    sections: [
      { h2: "세 단계로 이용 시작", blocks: [
        { type: "steps", items: [
          "apiToken.sale 계정을 만들고 sk-pool 키를 발급합니다.",
          "카드 또는 암호화폐로 정수 달러 금액을 충전합니다. 사용자 측 별도 Kimi plan은 필요 없습니다.",
          "GET https://router.apitoken.sale/v1/models를 읽고 키의 live catalog가 노출하는 kimi/* ID를 선택합니다.",
        ] },
        sourceBlock("how-to-buy-kimi-api-key", 0, 1),
      ] },
      { h2: "Kimi route의 차이", blocks: [
        { type: "list", items: [
          "Kimi는 별도 provider namespace이지만 네 번째 wire format은 아닙니다. POST /v1/messages와 x-api-key 또는 unified OpenAI 호환 /v1 route를 사용합니다.",
          "공개 ID는 kimi/k3, kimi/kimi-for-coding 같은 subscription alias이며 내부 tariff 모델명이 아닙니다.",
          "K3에는 256K와 1M context 표기가 있고 Kimi for Coding에는 기본과 High Speed alias가 있습니다.",
          "모델 availability는 provider capacity와 key policy에 따라 달라질 수 있어 live /v1/models가 권위입니다.",
        ] },
      ] },
    ],
    faq: [
      { q: "Kimi에 별도 API 키가 필요한가요?", a: "아니요. 같은 sk-pool 키와 잔액이 Kimi와 다른 지원 provider를 포함합니다." },
      { q: "Kimi는 어떤 endpoint를 사용하나요?", a: "Anthropic Messages에는 https://router.apitoken.sale/v1/messages를, OpenAI 호환 client에는 /v1 Chat Completions를 사용합니다. 둘 다 공개 kimi/* ID를 받습니다." },
      { q: "왜 /v1/models를 먼저 확인해야 하나요?", a: "catalog가 key-scoped이므로 현재 routing과 pricing이 가능한 모델만 반환합니다." },
    ],
  },
  "kimi-api-quickstart": {
    title: "Kimi API 빠른 시작",
    h1: "Anthropic SDK로 Kimi API 빠르게 시작하기",
    description: "apiToken.sale에서 Kimi K3와 Kimi for Coding을 호출하세요. Anthropic Messages, x-api-key, namespaced model ID, terminal usage, 공유 잔액을 설명합니다.",
    keywords: ["kimi api 빠른 시작", "kimi api 튜토리얼", "kimi anthropic api", "kimi k3 api 예제", "kimi for coding api", "kimi api curl"],
    dek: "Kimi는 unified router에서 Anthropic Messages protocol을 사용합니다. 기존 Anthropic client에는 custom base URL, apiToken.sale 키, 명시적 kimi/* model ID만 필요합니다.",
    sections: [
      { h2: "curl로 첫 요청", blocks: [
        sourceBlock("kimi-api-quickstart", 0, 0),
        { type: "p", text: "terminal usage가 Anthropic 형식이므로 기존 usage parser를 그대로 사용할 수 있습니다. route는 stream: true를 받지만 provider boundary의 증분성은 아직 live 검증 중입니다." },
      ] },
      { h2: "Anthropic Python SDK 사용", blocks: [
        sourceBlock("kimi-api-quickstart", 1, 0),
        { type: "note", text: "kimi-k2.7-code 같은 Open Platform ID로 바꾸지 마세요. public router는 GET /v1/models의 subscription alias를 받으며 OpenAI 호환 client도 unified /v1 route에서 같은 Kimi alias를 호출합니다." },
      ] },
    ],
    faq: [
      { q: "Anthropic SDK로 Kimi를 호출할 수 있나요?", a: "네. base_url을 https://router.apitoken.sale로 설정하고 key-scoped catalog의 kimi/* model ID를 선택하세요." },
      { q: "Kimi route에 stream: true를 설정할 수 있나요?", a: "route는 이 parameter를 받지만 upstream과 public chunk의 증분성은 아직 live 검증 중입니다. chunk 도착 timing이 중요하면 non-stream mode를 사용하세요." },
      { q: "어떤 model ID로 시작해야 하나요?", a: "coding 기본값은 kimi/kimi-for-coding, full 1M 없이 K3 reasoning이 필요하면 kimi/k3-256k가 적합합니다." },
    ],
  },
  "kimi-api-pricing": {
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
  },
  "kimi-k3-vs-kimi-for-coding": {
    title: "Kimi K3 vs Kimi for Coding",
    h1: "Kimi K3와 Kimi for Coding 비교",
    description: "Kimi K3, K3 256K, Kimi for Coding, High Speed를 context, reasoning control, latency, token 가격으로 비교합니다.",
    keywords: ["kimi k3 vs kimi for coding", "kimi k3 api", "kimi k2.7 code", "최고의 kimi 코딩 모델", "kimi 모델 비교", "kimi highspeed"],
    dek: "K3는 reasoning과 long context 제품군이고 Kimi for Coding은 경제적인 coding 제품군입니다. High Speed는 두 배 rate로 latency를 낮추며 K3 alias는 256K 또는 1M mode를 선택합니다.",
    sections: [
      { h2: "모델 제품군 맵", blocks: [
        { type: "table", headers: ["공개 ID", "Context", "적합한 작업"], rows: [
          ["kimi/kimi-for-coding", "256K", "일상 coding과 경제적 agent loop"],
          ["kimi/kimi-for-coding-highspeed", "256K", "속도가 비용을 정당화하는 latency-sensitive coding"],
          ["kimi/k3-256k", "256K", "full context가 필요 없는 K3 reasoning"],
          ["kimi/k3 · kimi/k3[1m]", "1M", "대형 codebase, document, 어려운 reasoning"],
        ] },
        { type: "p", text: "k3[1m]은 K3 1M mode의 compatibility spelling이며 별도 모델이 아닙니다. router가 provider의 실제 k3 wire model로 normalize합니다." },
      ] },
      { h2: "Reasoning과 routing", blocks: [
        { type: "list", items: [
          "K3는 low, high, max reasoning effort를 지원하며 기본은 high입니다.",
          "Kimi for Coding과 High Speed는 thinking이 켜져 있습니다.",
          "alias를 고정하기 전 key-scoped /v1/models를 확인합니다.",
          "실용적 router는 일상 code를 Kimi for Coding으로, 크고 어려운 작업을 K3로 보냅니다.",
        ] },
      ] },
    ],
    faq: [
      { q: "coding에 가장 좋은 Kimi 모델은?", a: "Kimi for Coding이 경제적인 기본입니다. 어려운 reasoning이나 long context에는 K3, 두 배 가격보다 낮은 latency가 중요할 때만 High Speed를 사용하세요." },
      { q: "k3와 k3[1m]은 다른 모델인가요?", a: "아니요. 같은 K3 1M mode를 선택하며 bracket 형식은 compatibility alias입니다." },
      { q: "내부 official model ID를 요청할 수 있나요?", a: "아니요. kimi-k2.7-code 같은 tariff ID가 아니라 router catalog의 공개 subscription alias를 사용하세요." },
    ],
  },
  "kimi-api-for-opencode": {
    title: "OpenCode에서 Kimi API 사용",
    h1: "OpenCode에서 Kimi K3와 Kimi for Coding 실행",
    description: "router plugin, key-scoped model catalog, 명시적 kimi/* ID, 하나의 선불 API 키로 OpenCode를 Kimi에 연결하세요.",
    keywords: ["kimi opencode", "kimi api opencode", "kimi k3 opencode", "kimi for coding 설정", "opencode custom provider", "kimi coding agent"],
    dek: "OpenCode는 Kimi namespace를 명시적으로 지정하고 router의 live catalog를 사용합니다. 수동으로 provider limit을 유지하지 않고 K3와 Kimi for Coding을 전환하기에 안전한 coding-agent 설정입니다.",
    sections: [
      { h2: "설치와 검증", blocks: [
        { type: "steps", items: [
          "apiToken.sale OpenCode installer를 실행합니다. router plugin을 기존 config에 merge하고 backup을 남깁니다.",
          "OpenCode를 재시작해 plugin이 key-scoped model catalog를 받게 합니다.",
          "명시적 namespaced model로 결정적 prompt 하나를 실행합니다.",
        ] },
        sourceBlock("kimi-api-for-opencode", 0, 1),
      ] },
      { h2: "Kimi 모델 안전하게 선택", blocks: [
        { type: "list", items: [
          "apitoken/kimi/kimi-for-coding — 경제적인 coding 기본.",
          "apitoken/kimi/kimi-for-coding-highspeed — 두 배 token rate로 더 낮은 latency.",
          "apitoken/kimi/k3-256k — 더 작은 context mode의 K3 reasoning.",
          "apitoken/kimi/k3 — catalog가 노출할 때 full 1M K3.",
        ] },
        { type: "note", text: "Claude Code와 Kimi Code도 Kimi를 지원하지만 설정이 다릅니다. Claude Code는 모든 model tier를 pin해야 하고 Kimi Code는 명시적 OpenAI 호환 provider block을 사용합니다." },
      ] },
    ],
    faq: [
      { q: "OpenCode가 Kimi를 지원하나요?", a: "네. apiToken.sale router plugin이 live Kimi namespace를 등록하고 모델을 apitoken/kimi/{model}로 선택합니다." },
      { q: "static model list보다 plugin이 좋은 이유는?", a: "ID, limit, availability를 key-scoped live catalog와 맞춰 retired 또는 unavailable alias가 local config에 남지 않습니다." },
      { q: "Claude Code도 Kimi를 사용할 수 있나요?", a: "네. 다른 설정으로 가능합니다. Claude Code를 Anthropic endpoint에 연결하고 main, Opus, Sonnet, Haiku, subagent model variables를 하나의 Kimi alias로 pin하세요." },
    ],
  },
  "kimi-api-for-claude-code": {
    title: "Claude Code에서 Kimi K3 사용",
    h1: "Claude Code에서 Kimi K3와 Kimi for Coding 실행",
    description: "apiToken.sale를 통해 Claude Code에 Kimi K3 또는 Kimi for Coding을 설정하세요. 모든 model tier를 pin하고 1M context를 유지하며 endpoint를 검증합니다.",
    keywords: ["kimi claude code", "kimi k3 claude code", "kimi for coding claude code", "claude code custom model", "claude code kimi api", "k3 1m claude code"],
    dek: "Claude Code는 이미 Anthropic Messages를 사용하므로 Kimi를 직접 실행할 수 있습니다. 안정적인 설정은 모든 내부 model tier를 하나의 Kimi alias로 pin합니다. 그렇지 않으면 main session은 동작해도 subagent가 상속한 Claude model에서 실패할 수 있습니다.",
    sections: [
      { h2: "연결과 모든 model tier pin", blocks: [
        sourceBlock("kimi-api-for-claude-code", 0, 0),
        { type: "p", text: "Anthropic route에서는 bare subscription alias를 사용합니다. k3-256k 또는 kimi-for-coding 같은 256K 모델에는 tier pin을 유지하되 두 개의 1M context 변수는 생략합니다." },
      ] },
      { h2: "모델 소개가 아니라 route 검증", blocks: [
        { type: "list", items: [
          "/status를 열어 Anthropic base URL이 apiToken.sale인지 확인합니다.",
          "모델에게 정체를 묻지 마세요. Claude Code system prompt 때문에 어떤 backend도 Claude라고 답할 수 있습니다.",
          "none/off는 다른 model 선택이 아니라 K3 reasoning 비활성화로 취급하세요. live coverage에서도 K3 tariff가 유지됐고 kimi-k2.6은 public addressable model이 아닙니다.",
          "alias를 장기 pin하기 전에 GET /v1/models를 확인합니다.",
        ] },
      ] },
    ],
    faq: [
      { q: "Claude Code가 Kimi K3를 지원하나요?", a: "네. Claude Code를 https://router.apitoken.sale로 연결하고 모든 model tier를 허용된 Kimi subscription alias에 pin하세요." },
      { q: "왜 모든 Claude Code model variable을 pin해야 하나요?", a: "Claude Code는 main session, tier, subagent 모델을 따로 선택합니다. pin되지 않은 tier는 Claude ID를 상속해 해당 background path가 실행될 때만 실패할 수 있습니다." },
      { q: "Claude Code에서 K3 full 1M context를 유지하려면?", a: "k3 또는 k3[1m]을 사용하고 CLAUDE_CODE_MAX_CONTEXT_TOKENS와 CLAUDE_CODE_AUTO_COMPACT_WINDOW를 모두 1048576으로 설정하세요." },
    ],
  },
  "kimi-api-for-kimi-code": {
    title: "Kimi Code에서 apiToken.sale 사용",
    h1: "Kimi Code에서 Kimi, Claude, GPT, Gemini 실행",
    description: "OpenAI 호환 provider config로 Kimi Code를 apiToken.sale에 연결하고 namespaced model을 선언하며 config.toml의 API 키를 보호하세요.",
    keywords: ["kimi code api", "kimi code custom provider", "kimi code config toml", "kimi code api 키", "kimi code k3", "kimi code openai 호환"],
    dek: "Kimi Code는 custom OpenAI 호환 provider를 받으므로 하나의 apiToken.sale provider entry로 unified catalog에 접근할 수 있습니다. 각 모델은 실제 namespace와 검증된 context window로 별도 선언해야 합니다.",
    sections: [
      { h2: "설치하고 provider 선언", blocks: [
        sourceBlock("kimi-api-for-kimi-code", 0, 0),
        { type: "note", text: "/login을 실행하지 마세요. CLI가 Kimi membership에 연결됩니다. Kimi Code는 custom-provider credential을 config.toml에만 저장하므로 파일에 plain text 키가 들어가며 권한을 제한해야 합니다." },
      ] },
      { h2: "실행, 검증, 모델 추가", blocks: [
        sourceBlock("kimi-api-for-kimi-code", 1, 0),
        { type: "list", items: [
          "/status에 provider base URL이 https://router.apitoken.sale/v1로 표시되어야 합니다.",
          "model field는 kimi/k3, openai/gpt-5.6-terra, google/gemini-3.6-flash 같은 unified catalog namespace를 사용합니다.",
          "추가 모델마다 검증된 max_context_size를 config.toml에 선언하세요. Kimi Code가 이 값으로 context compact 시점을 결정합니다.",
        ] },
      ] },
    ],
    faq: [
      { q: "Kimi Code가 apiToken.sale 키를 사용할 수 있나요?", a: "네. base_url이 https://router.apitoken.sale/v1인 OpenAI 호환 provider를 추가하고 Kimi Code config.toml에 키를 저장하세요." },
      { q: "Kimi Code에서 Kimi 외 모델도 실행할 수 있나요?", a: "네. 같은 provider entry로 unified catalog에 접근하며 각 Claude, GPT, Gemini, Kimi 모델을 namespaced ID와 올바른 context limit으로 선언합니다." },
      { q: "chmod 600이 왜 중요한가요?", a: "Kimi Code는 shell에서 custom-provider credential을 읽지 않습니다. raw API 키가 config.toml에 있으므로 계정 소유자만 읽을 수 있어야 합니다." },
    ],
  },
};
