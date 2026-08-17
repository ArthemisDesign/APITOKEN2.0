import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
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
  };
