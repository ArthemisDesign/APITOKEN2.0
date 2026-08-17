import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "Nano Banana 2 API 指南",
    h1: "使用 Nano Banana 2 API 生成图像",
    description: "通过原生 Gemini API 使用 Gemini 3.1 Flash Image（Nano Banana 2）：准确 model ID、generateContent、图像输出定价和固定五折。",
    keywords: ["nano banana 2 api", "gemini 3.1 flash image api", "gemini 图像生成 api", "nano banana api 密钥", "gemini 图像价格", "google 图像 api"],
    dek: "Nano Banana 2 是 Gemini 3.1 Flash Image 的公开名称。它使用原生 generateContent，接受多模态输入，并与文本模型共用余额返回渲染图像。",
    sections: [
      { h2: "使用准确 model ID", blocks: [
        sourceBlock("nano-banana-2-api-guide", 0, 0),
        { type: "p", text: "按 MIME type 解析返回 parts：文本 part 是说明，图像 part 是渲染资产。API 中使用 gemini-3.1-flash-image，而不是营销昵称。" },
      ] },
      { h2: "限制与价格", blocks: [
        { type: "list", items: [
          "128K 上下文，最多 32K 输出，小于文本 Flash 系列。",
          "官方文本输入/输出为 $0.50/$3 每百万，图像输出为 $60。",
          "apiToken.sale 五折后为 $0.25/$1.50，图像输出 $30。",
          "该图像模型的缓存输入仍按完整 $0.50 输入费率计费。",
        ] },
        { type: "note", text: "只需文本时使用文本 Flash。只有响应必须包含渲染图像时才使用 Flash Image，其图像输出单独计费。" },
      ] },
    ],
    faq: [
      { q: "Nano Banana 2 的 API model ID 是什么？", a: "原生 Gemini generateContent 路由上的 gemini-3.1-flash-image。" },
      { q: "Nano Banana 2 图像输出多少钱？", a: "官方 $60/百万 image-output token，apiToken.sale 固定五折后 $30。" },
      { q: "需要单独图像 API 密钥吗？", a: "不需要。使用同一 sk-pool 密钥放在 x-goog-api-key 中，并共享预付余额。" },
    ],
  };
