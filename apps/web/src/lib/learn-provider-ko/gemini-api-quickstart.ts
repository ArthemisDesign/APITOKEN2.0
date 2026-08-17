import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
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
  };
