import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "Gemini API 定价详解：Pro、Flash、Flash-Lite 全费率",
    h1: "Gemini API 定价：Pro、Flash、Flash-Lite 与图像输出",
    description: "Gemini API 定价完整拆解：Pro、Flash、Flash-Lite 的 token 费率、缓存输入、200K 长上下文加价、Nano Banana 2 图像输出，以及 apiToken.sale 固定五折。",
    keywords: ["gemini api 定价", "gemini api 价格", "gemini api 每 token 成本", "gemini 3.6 flash 价格", "gemini 3.1 pro 价格", "gemini flash lite 价格", "gemini 缓存输入 计费", "gemini 长上下文 价格", "nano banana 2 api 价格", "gemini 图像输出 费用", "最便宜的 gemini 模型", "gemini api 每百万 token 价格", "便宜 gemini api"],
    dek: "Gemini API 定价由三条独立计量的计费项构成——输入、缓存输入和输出：费率按模型档位划分，Pro 有长上下文加价，Nano Banana 2 另有一条图像输出计费项。本文列出全部当前费率、各计费项的叠加算法，以及 apiToken.sale 固定 50% 折扣在结算环节的生效位置。",
    sections: [
      { h2: "Gemini API 每 100 万 token 价格：逐模型对比", blocks: [
        { type: "p", text: "Gemini API 定价是纯粹的按 token 计量：你为发送的输入 token 和模型生成的输出 token 付费，缓存输入作为一条更便宜的独立计费项结算，没有按请求收费，也没有最低消费。官方费率从 Gemini 2.5 Flash-Lite 的每 100 万 token $0.10/$0.40 到 Gemini 3.1 Pro Preview 的 $2/$12 不等。apiToken.sale 对每一条计费项都按固定五折结算，同样的请求只需 $0.05/$0.20 到 $1/$6。" },
        { type: "table", headers: ["模型", "官方 输入 / 缓存 / 输出", "本站五折后价格"], rows: [
          ["gemini-3.1-pro-preview", "$2 / $0.20 / $12", "$1 / $0.10 / $6"],
          ["gemini-3.6-flash", "$1.50 / $0.15 / $7.50", "$0.75 / $0.075 / $3.75"],
          ["gemini-3.1-flash-lite", "$0.25 / $0.025 / $1.50", "$0.125 / $0.0125 / $0.75"],
          ["gemini-2.5-flash-lite", "$0.10 / $0.01 / $0.40", "$0.05 / $0.005 / $0.20"],
        ] },
        { type: "p", text: "以上均为每 100 万 token 的价格。缓存输入是一条独立的用量计费项，会在响应的 usage 元数据中单独上报：在文本模型上它按新输入费率的 10% 计费，对重复的提示词前缀自动生效，且没有单独的缓存写入费用。同一个 token 绝不会同时被按缓存输入和新输入重复计费。" },
      ] },
      { h2: "一次真实调用的三条 token 计费项如何叠加", blocks: [
        { type: "p", text: "任何一次调用的成本都是三项乘积之和：新输入 token 数乘以输入费率，缓存 token 数乘以缓存费率，输出 token 数乘以输出费率。一次向 gemini-3.6-flash 发送 20,000 个输入 token（其中 12,000 个命中缓存）、生成 1,500 个输出 token 的请求，按官方费率为 8,000 × $1.50/M + 12,000 × $0.15/M + 1,500 × $7.50/M = $0.012 + $0.0018 + $0.011 ≈ $0.025。固定五折之后，这次调用实际结算约 $0.0125。" },
        { type: "list", items: [
          "输出是最贵的一条计费项：上面每一个文本模型的输出费率都是输入的 4–6 倍，所以啰嗦的回答比冗长的提示词更烧钱。",
          "选对模型胜过裁剪提示词：3.1 Flash-Lite 的输入价格是 3.1 Pro 的八分之一，2.5 Flash-Lite 则是二十分之一。",
          "稳定的前缀——系统提示词、工具 schema、few-shot 示例——命中缓存后按 10% 计费，重复流量不改代码也能越来越便宜。",
        ] },
        { type: "note", text: "每个 generateContent 响应里的 usageMetadata 对象会分别上报提示词、缓存和候选输出的 token 数。做预算要以这些权威数字为准，而不是数提示词的字符数。" },
      ] },
      { h2: "输入超过 200K token 的长上下文定价", blocks: [
        { type: "p", text: "Gemini 3.1 Pro Preview 是唯一有长上下文加价的文本模型。一旦请求的输入超过 200K token，整个请求——不只是超出阈值的那部分——都按每 100 万 $4 输入、$0.40 缓存输入、$18 输出计费：输入费率翻倍，输出费率变为 1.5 倍。" },
        { type: "p", text: "Flash 和 Flash-Lite 没有这一档。它们在整个 100 万 token 上下文窗口内（输出上限 64K）都保持标准费率。一个 50 万 token 的分析任务，仅输入在 Pro 上就要 $2，在 gemini-3.6-flash 上只要 $0.75——然后五折再把这两个数字各砍一半。" },
        { type: "note", text: "把超大上下文发给 Pro 之前先量一下：countTokens 可以免费返回精确的计费输入 token 数（见下文），这样你可以有意识地让超规格任务走 Flash，而不是事后在仪表盘上才发现这笔加价。" },
      ] },
      { h2: "图像输出：Gemini 3.1 Flash Image（Nano Banana 2）", blocks: [
        { type: "p", text: "Nano Banana 2 是 gemini-3.1-flash-image 的公开名称，它的定价方式不同于文本模型。文本进出都很便宜——每 100 万 token 输入 $0.50、文本输出 $3——但渲染出的图像作为独立计费项，按每 100 万图像输出 token $60 收费。上下文窗口也更小：输入 128K，输出最多 32K。" },
        sourceBlock("gemini-api-pricing", 3, 1),
        { type: "list", items: [
          "图像输出按图像输出 token 计量：官方每 100 万 $60，五折后 $30。",
          "这个模型的缓存输入没有折扣——按完整的 $0.50 输入费率计费。",
          "同一响应里的文本输出仍按标准的每 100 万 $3 计费。",
        ] },
        { type: "link", text: "模型详情：上下文、输出上限与每一条费率", href: "/models/gemini-3-1-flash-image" },
      ] },
      { h2: "生成前用 countTokens 预估花费", blocks: [
        { type: "p", text: "countTokens 是同一模型路径上的免费调用。它返回该请求将被计费的精确输入 token 数，不生成任何内容，也不占用配额或余额。在跑大型 Pro 任务或图像任务之前先调它，长上下文加价就永远不会让你措手不及。" },
        sourceBlock("gemini-api-pricing", 4, 1),
        { type: "p", text: "流式也不改变价格：streamGenerateContent?alt=sse 计量的 token 计费项与一次性 generateContent 调用完全相同，所以按延迟和体验选传输方式即可，不用考虑成本差异。" },
      ] },
      { h2: "apiToken.sale 如何按五折结算 Gemini 用量", blocks: [
        { type: "p", text: "上述计量方式在 apiToken.sale 上没有任何变化。你的请求运行在原生 Gemini /v1beta generateContent 接口上，用 x-goog-api-key 请求头里的密钥鉴权，usage 元数据上报的 token 数也完全一致。变化的只有结算：每次调用先折算成精确的 Google 官方花费，再减去固定 50% B2C 折扣，只有折后净额从你的预付余额中扣除。没有订阅费、没有席位费、没有加价。" },
        { type: "p", text: "同一把密钥和同一个余额覆盖支持范围内的 Claude、GPT、Gemini 和 Kimi 模型，每个模型按各自的官方费率表计量，享受同样的折扣。每个请求在仪表盘里都有 token 级明细，你可以拿本文的算法和自己的真实流量逐笔对账。" },
        { type: "link", text: "全部支持模型及其每 token 费率", href: "/models" },
      ] },
    ],
    faq: [
      { q: "最便宜的 Gemini API 模型是哪个？", a: "Gemini 2.5 Flash-Lite：官方每 100 万 token 输入 $0.10、输出 $0.40，缓存输入 $0.01。叠加 apiToken.sale 固定五折后是 $0.05/$0.20——已公开的 Gemini 每 token 最低价。" },
      { q: "Gemini 的长上下文定价什么时候生效？", a: "只在 Gemini 3.1 Pro Preview 上、输入超过 200K token 时生效。届时整个请求按每 100 万 $4/$0.40/$18 计费——输入 2 倍、输出 1.5 倍。Flash 和 Flash-Lite 在整个 100 万 token 窗口内都保持标准费率。" },
      { q: "Gemini 图像输出多少钱？", a: "Gemini 3.1 Flash Image（Nano Banana 2）渲染输出官方按每 100 万图像输出 token $60 计费，固定五折后为 $30。同一响应中的文本输出按每 100 万 $3 计费。" },
      { q: "缓存输入会在新输入之上叠加收费吗？", a: "不会。缓存 token 是独立的计费项，在文本模型上按输入费率的 10% 计费，并在 usageMetadata 中单独上报——同一批 token 绝不会被重复计费。例外是 gemini-3.1-flash-image，它的缓存输入按完整输入费率计费。" },
      { q: "五折对长上下文和图像计费项也有效吗？", a: "有效。apiToken.sale 先按官方费率精确计算每一条计费项的花费——输入、缓存输入、输出、Pro 长上下文加价和图像输出——然后从总额中减去 50%，再扣你的预付余额。" },
      { q: "可以免费查一个提示词的 token 数吗？", a: "可以。向 /v1beta/models/{model}:countTokens 发 POST 请求，密钥放在 x-goog-api-key 请求头里；它会返回精确的输入 token 数，且不碰你的余额。" },
    ],
  };
