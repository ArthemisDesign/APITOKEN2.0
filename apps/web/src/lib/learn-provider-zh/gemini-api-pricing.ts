import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Gemini API 定价详解",
    h1: "Gemini API 定价：Pro、Flash、Flash-Lite 与图像输出",
    description: "比较 Gemini Pro、Flash、Flash-Lite 和 Nano Banana 2 的价格，包括缓存输入、长上下文、图像输出和 apiToken.sale 固定五折。",
    keywords: ["gemini api 定价", "gemini api 成本", "gemini token 价格", "gemini flash 价格", "gemini pro 价格", "便宜 gemini api"],
    dek: "Gemini 价格取决于模型层级、缓存输入、输出模态，以及 Pro 的上下文长度。网关精确结算官方计费项，再应用 50% 折扣。",
    sections: [
      { h2: "代表性文本模型费率", blocks: [
        { type: "table", headers: ["模型", "官方输入 / 缓存 / 输出", "五折后价格"], rows: [
          ["gemini-3.1-pro-preview", "$2 / $0.20 / $12", "$1 / $0.10 / $6"],
          ["gemini-3.6-flash", "$1.50 / $0.15 / $7.50", "$0.75 / $0.075 / $3.75"],
          ["gemini-3.1-flash-lite", "$0.25 / $0.025 / $1.50", "$0.125 / $0.0125 / $0.75"],
          ["gemini-2.5-flash-lite", "$0.10 / $0.01 / $0.40", "$0.05 / $0.005 / $0.20"],
        ] },
        { type: "p", text: "以上均为每 100 万 token。缓存输入是提供商报告的独立 usage 项，不会与同一批新输入 token 重复相加。" },
      ] },
      { h2: "长上下文与图像", blocks: [
        { type: "list", items: [
          "Gemini 3.1 Pro Preview 输入超过 200K 后，整次请求按 $4 输入/$18 输出每 100 万计费。",
          "Gemini 3.1 Flash Image 的文本输出为 $3，图像输出为 $60 每 100 万 image token。",
          "Flash Image 的缓存输入按完整输入费率收费，不享受文本模型缓存折扣。",
          "精确计算官方项目后，再应用固定 50% B2C 折扣。",
        ] },
      ] },
    ],
    faq: [
      { q: "最便宜的 Gemini 模型是什么？", a: "在已发布文本层级中，Gemini 2.5 Flash-Lite 官方为 $0.10 输入/$0.40 输出，五折后为 $0.05/$0.20。" },
      { q: "Gemini 长上下文何时加价？", a: "Gemini 3.1 Pro Preview 输入超过 200K token 时，整次请求使用更高输入、缓存和输出费率。" },
      { q: "Gemini 图像输出如何计费？", a: "Gemini 3.1 Flash Image 官方为 $60/百万 image-output token，五折后为 $30。" },
    ],
  };
