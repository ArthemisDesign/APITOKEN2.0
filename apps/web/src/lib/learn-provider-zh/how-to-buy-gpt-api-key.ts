import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "如何购买 GPT API 密钥",
    h1: "如何购买 GPT API 密钥",
    description: "购买 GPT API 密钥：预付余额，支持银行卡或加密货币付款。一个 OpenAI 兼容端点即可使用 GPT-5.6 Sol、Terra、Luna，GPT-5.5 和 GPT Image 2，按官方费用五折结算。",
    keywords: ["购买 gpt api 密钥", "gpt api 密钥", "gpt api 密钥怎么买", "openai api key 购买", "gpt-5.6 api 接入", "openai 兼容 api 密钥", "gpt api 预付费", "无 openai 账号的 gpt api 密钥", "gpt api 加密货币付款", "便宜的 gpt-5.6 api"],
    dek: "不开 OpenAI Platform 账号也能买到 GPT API 密钥：注册 apiToken.sale 账号，用银行卡或加密货币给预付余额充值，生成一把 sk-pool 密钥。这把密钥以 Authorization: Bearer 对 OpenAI 兼容端点认证，按官方 token 费用的五折提供 GPT-5.6、GPT-5.5 和 GPT Image 2。本文走完整个购买流程、计费算术和精确的客户端配置。",
    sections: [
      { h2: "购买流程：账号、余额、密钥", blocks: [
        { type: "steps", items: [
          "创建 apiToken.sale 账号，支持 Google、GitHub 或邮箱加密码注册。用 Google 和 GitHub 注册的账号自带 $5 平台赠金，可用于支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码注册的账号没有赠金。",
          "用银行卡或加密货币充值任意整数美元金额。没有固定套餐，也没有按月承诺——余额是预付费的，只有请求实际运行时才会扣。",
          "在控制台生成 API 密钥。密钥形如 sk-pool-…，立即生效；同一把密钥和同一份余额也覆盖支持的 Claude、Gemini 和 Kimi 模型。",
          "接入项目之前，先用下面的 curl 验证密钥。返回 200 且有真实输出就闭环了；401 说明密钥或请求头名写错了。",
        ] },
        sourceBlock("how-to-buy-gpt-api-key", 0, 1),
        { type: "p", text: "在这里买 GPT API 密钥就三步——注册、充值、生成——密钥在下一次请求时即刻可用：全程不需要 OpenAI Platform 账号，没有候补名单，也没有任何人工审核。上面的 curl 只要返回了输出文本，密钥、余额、端点这整条链路就被端到端验证了。" },
      ] },
      { h2: "这把密钥能用哪些 GPT 模型", blocks: [
        { type: "p", text: "你的密钥实际可用的模型目录，永远以 GET https://router.apitoken.sale/v1/models 的实时应答为准——以它为准，而不要想当然地认为 OpenAI 文档里的某个模型名在这里也存在。目前的阵容覆盖三个 GPT-5.6 层级、两个上一代模型，以及独立的 GPT Image 2 生成与编辑路由：" },
        { type: "table", headers: ["模型 ID", "层级", "官方 输入 / 缓存 / 输出（每 1M token，$）", "五折后"], rows: [
          ["gpt-5.6-sol（别名：gpt-5.6）", "旗舰", "$5 / $0.50 / $30", "$2.50 / $0.25 / $15"],
          ["gpt-5.6-terra", "均衡", "$2 / $0.20 / $12", "$1 / $0.10 / $6"],
          ["gpt-5.6-luna", "快速", "$0.20 / $0.02 / $1.20", "$0.10 / $0.01 / $0.60"],
          ["gpt-5.5", "上一代旗舰", "$5 / — / $30", "$2.50 / — / $15"],
          ["gpt-5.4", "上一代均衡", "$2.50 / — / $15", "$1.25 / — / $7.50"],
        ] },
        { type: "list", items: [
          "三个 GPT-5.6 层级共享 400K 上下文窗口、最高 128K 输出、文本和图像输入，以及可调的推理强度——从 none 到 xhigh，GPT-5.6 系列还支持 max。",
          "Responses 和 Chat Completions 都支持增量 SSE 流式输出，现有的生成或流式循环无需结构性改动即可迁移。",
          "GPT Image 2 走独立的生成和编辑路由，而不是聊天接口；费用从同一份余额扣除。",
        ] },
        { type: "link", text: "各模型规格与折后价格", href: "/models" },
        { type: "link", text: "GPT 定价完整拆解：缓存写入与长上下文", href: "/docs/learn/gpt-api-pricing" },
      ] },
      { h2: "预付余额与五折如何结算", blocks: [
        { type: "p", text: "没有订阅，也没有月费。每个请求先按 OpenAI 官方 token 费率计量，再减去你固定的 50% B2C 折扣，净额从预付余额中扣除——所以 $50 余额可以覆盖 $100 的官方价用量。控制台会记录每次请求结算后的 token 用量和精确的折后扣费。" },
        { type: "list", items: [
          "缓存输入单独计价，远比新鲜输入便宜（旗舰模型上每 1M 是 $0.50 对 $5），所以在多次调用之间保持稳定的提示词前缀，省的是真金白银。",
          "缓存写入按普通输入的 125% 计费；缓存读取按输入的 10% 计费。",
          "推理 token 计入输出用量，不会作为独立计费项再收第二次。",
        ] },
        { type: "note", text: "输入 token 超过 272K 后，长上下文费率对整个请求生效——输入 2 倍、输出 1.5 倍，且在折扣之前。273K 的请求比 270K 贵一倍还多；在越过边界之前，先拆分超大上下文。" },
        { type: "note", text: "余额耗尽后，请求会以余额不足错误失败，直到你再次充值——没有透支，也不会从你的卡里冒出意外扣款。" },
      ] },
      { h2: "配置官方 SDK 与现有客户端", blocks: [
        { type: "p", text: "每个 OpenAI 兼容客户端只需要改两个值：base URL 和凭证。提示词、流式代码和工具定义原样保留——官方 SDK 无需改动即可工作：" },
        sourceBlock("how-to-buy-gpt-api-key", 3, 1),
        { type: "p", text: "需要经典 Chat Completions 形态的框架——旧的 LangChain 链、LiteLLM 配置、大多数开源聊天 UI——在同一个主机上用同一把密钥和同样的模型 ID 即可工作；只需把方法名换成 client.chat.completions.create，并传入 messages 数组。" },
        { type: "note", text: "把密钥放在服务端环境变量里，绝不要写进客户端代码或提交进仓库的文件。GPT 调用用 Authorization: Bearer 认证；x-api-key 属于 Anthropic Messages 通道，x-goog-api-key 属于 Gemini 原生通道——在这里发任何一个都会返回 401。" },
      ] },
      { h2: "这个网关是什么——以及不是什么", blocks: [
        { type: "p", text: "这是一个独立的 OpenAI 兼容网关，有自己的账号、预付余额和支持模型目录——不是 OpenAI Platform。对纯文本和视觉聊天负载来说，接口面是完整的：模型发现、Responses、Chat Completions、流式输出，以及 GPT Image 2 路由。音频、文件、realtime、assistants、batch 和微调端点不可用；依赖这些能力的应用不适合迁移。" },
        { type: "p", text: "错误以标准的 OpenAI 错误信封返回，现有的错误处理代码可以继续工作。集成过程中你几乎只会遇到三个状态码：" },
        { type: "list", items: [
          "401——密钥缺失或拼错，或者你把 Authorization: Bearer 写成了 x-api-key。在应用之外用 curl 复现，定位是哪一半坏了。",
          "402——预付余额需要充值；重试和退避都救不了空余额。",
          "404——该模型 ID 未在你的密钥上启用；去查 GET https://router.apitoken.sale/v1/models，不要凭假设。",
        ] },
        { type: "link", text: "从密钥到首个流式响应：完整快速上手", href: "/docs/learn/openai-api-quickstart" },
      ] },
    ],
    faq: [
      { q: "买这把 GPT API 密钥需要 OpenAI 账号吗？", a: "不需要。密钥、余额和计费都来自 apiToken.sale；兼容的 GPT 客户端只需要自定义 base URL 和 Bearer 密钥。没有候补名单，也没有人工审核。" },
      { q: "可以用加密货币支付 GPT API 密钥吗？", a: "可以。结账支持银行卡和加密货币，充值金额为任意整数美元，没有固定套餐。用 Google 或 GitHub 创建的新账号还自带 $5 平台赠金。" },
      { q: "一把密钥能同时跑 GPT 和 Claude 吗？", a: "能。同一把 sk-pool 密钥和同一份预付余额覆盖所有支持的提供商；只有端点和认证头随协议变化——OpenAI 兼容通道用 Bearer，Anthropic Messages 用 x-api-key。" },
      { q: "这里 GPT-5.6 每 1M token 多少钱？", a: "官方价 Sol 输入 $5、输出 $30，Terra $2/$12，Luna $0.20/$1.20；apiToken.sale 对这些计费项统一五折，所以 Terra 结算价是 $1/$6。" },
      { q: "这是 OpenAI Platform 吗？", a: "不是。这是一个独立的 OpenAI 兼容网关，有自己的账号、预付余额和支持模型目录。Responses、Chat Completions、流式输出和 GPT Image 2 可用；音频、realtime、assistants、batch 和微调不可用。" },
    ],
  };
