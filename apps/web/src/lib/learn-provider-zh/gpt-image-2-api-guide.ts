import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "GPT Image 2 API 指南",
    h1: "使用 GPT Image 2 API 生成和编辑图像",
    description: "通过 apiToken.sale 使用 GPT Image 2：准确端点、model ID、参考图限制、token 定价与固定五折。",
    keywords: ["gpt image 2 api", "gpt-image-2", "openai 图像生成 api", "gpt 图像编辑 api", "gpt image 价格", "图像生成 api"],
    dek: "GPT Image 2 使用独立图像路由，但与 GPT 文本模型共享 apiToken.sale 密钥和余额。可通过提示词生成图像，也可编辑最多五张 PNG 参考图。",
    sections: [
      { h2: "调用生成路由", blocks: [
        sourceBlock("gpt-image-2-api-guide", 0, 0),
        { type: "p", text: "编辑时向 /v1/images/edits 发送 multipart/form-data，使用同一模型并最多附带五张 PNG。当前接口每次返回一张非流式 PNG。" },
      ] },
      { h2: "图像计费方式", blocks: [
        { type: "table", headers: ["计费项", "官方每 100 万 token", "本站价格"], rows: [
          ["文本输入", "$5", "$2.50"],
          ["图像输入", "$8", "$4"],
          ["图像输出", "$30", "$15"],
        ] },
        { type: "list", items: [
          "缓存文本和图像输入按普通输入的 25% 计费。",
          "gpt-image-2 是固定快照 gpt-image-2-2026-04-21 的别名。",
          "图像 usage 与 GPT、Claude、Gemini 请求共用预付余额。",
        ] },
      ] },
    ],
    faq: [
      { q: "GPT Image 2 使用什么端点？", a: "新图像使用 POST /v1/images/generations，参考图编辑使用 POST /v1/images/edits。" },
      { q: "GPT Image 2 能编辑现有图像吗？", a: "可以。edits 路由通过 multipart/form-data 接受最多五张 PNG 参考图。" },
      { q: "需要单独的图像密钥或余额吗？", a: "不需要。它使用与其他模型相同的 Bearer 密钥和预付余额。" },
    ],
  };
