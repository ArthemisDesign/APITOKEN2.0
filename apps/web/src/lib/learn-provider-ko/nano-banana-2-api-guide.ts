import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
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
  };
