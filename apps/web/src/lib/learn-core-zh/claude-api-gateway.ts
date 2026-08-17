import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "什么是 Claude API 网关？",
    h1: "Claude API 网关是什么，什么时候需要它",
    description: "Claude API 网关位于你的工具和 Anthropic 之间，提供接入、计费和密钥管控。apiToken.sale 是原生网关，B2C 统一 50% 折扣。",
    keywords: ["claude api 网关", "什么是 api 网关", "anthropic api 网关", "claude api 代理", "claude 网关和代理的区别", "claude api 接入层", "claude api 原理", "claude api 价格", "claude api gateway", "anthropic api"],
    dek: "Claude API 网关一端接收标准的 Anthropic Messages API 请求，另一端把请求转发给模型提供方，中间加上认证、计费和密钥管理。你的工具感觉不出差别——但账单可以。本文讲清这一层如何工作，以及如何评判一个网关。",
    sections: [
      { h2: "Claude API 网关到底做什么", blocks: [
        { type: "p", text: "Claude API 网关是一个对你的代码讲 Anthropic Messages API、并把每个请求转发给上游模型的服务。你的工具指向网关，就和指向 api.anthropic.com 一模一样；请求之外的一切——谁在调用、花了多少钱、哪个密钥可以消费——都由网关负责。用它都是出于实际原因：价格更低、无需提供方账户即可即时开通，或者提供方本身没有的管控能力。" },
        { type: "list", items: [
          "对外呈现标准的 Anthropic Messages API，SDK 和工具无需改动即可使用。",
          "在请求发往上游之前认证你的密钥，并执行按密钥的管控规则。",
          "按提供方官方费率计量每个请求并处理计费——在这里是预付余额加 B2C 统一 50% 折扣。",
          "逐请求记录用量，附带 token 明细，可在控制台审计。",
        ] },
      ] },
      { h2: "网关、代理和直连的区别", blocks: [
        { type: "p", text: "大家经常把「网关」和「代理」混着用，但选型时这个区别很关键。反向代理只转发字节，对内容一无所知；网关理解协议，并掌管请求生命周期的一部分——认证、计量和结算。直连则意味着这一切都由 Anthropic 替你完成，按官方费率计费，需要 Anthropic 账户。" },
        { type: "table", headers: ["方式", "协议", "计费", "密钥管控"], rows: [
          ["Anthropic 直连", "原生 Messages API", "官方按 token 费率，由 Anthropic 出账", "控制台标准密钥"],
          ["普通反向代理", "代理转发什么就是什么", "没有自己的计费——计费留在上游", "无"],
          ["apiToken.sale 网关", "原生 Messages API，原封不动", "预付余额，比官方 B2C 花费统一低 50%", "每个密钥可选终身消费上限和到期日期"],
        ] },
        { type: "p", text: "一个实用的判断方法：如果把中间层删掉，唯一变化的是 base URL，那它就是代理；如果连计费、限额和用量历史也一起没了，那它就是网关。" },
      ] },
      { h2: "请求如何穿过网关", blocks: [
        { type: "steps", items: [
          "你的客户端向 https://router.apitoken.sale/v1/messages 发送标准 Messages API 请求，密钥放在 x-api-key 请求头里。",
          "网关认证密钥并检查管控规则——可选的终身消费上限和到期日期——然后才把请求路由到上游。",
          "模型生成回答。流式请求以标准的 Anthropic SSE 事件返回，逐 token 推送。",
          "用量按提供方官方费率计量，减去你的 B2C 统一 50% 折扣，净额从预付余额中扣减。",
          "请求带着模型和 token 级明细出现在控制台里，花费永远不会出乎意料。",
        ] },
        { type: "note", text: "把现有客户端切到网关，应该只需要改两样东西：base URL 和密钥。anthropic-version 请求头和模型 ID 保持不动。如果一个服务要求你改请求格式，那它是转译层，不是原生网关——流式、工具调用和提示词缓存上迟早会出现隐蔽的问题。" },
      ] },
      { h2: "原生协议，不是转译层", blocks: [
        { type: "p", text: "apiToken.sale 是 Anthropic 原生的：任何能对 api.anthropic.com 工作的客户端，都能逐字节原样对 https://router.apitoken.sale/v1/messages 工作。一个最小请求和 Anthropic 文档里写的完全一致：" },
        { type: "code", code: `curl https://router.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-sonnet-5",\n    "max_tokens": 1024,\n    "messages": [{"role": "user", "content": "Hello"}]\n  }'` },
        { type: "p", text: "同一把密钥不限于 Anthropic 这一条通道。它还能在 https://router.apitoken.sale/v1 上带 Authorization: Bearer 请求头讲 OpenAI 兼容协议，也能用 x-goog-api-key 讲 Gemini 原生协议——一把网关密钥覆盖支持的 Claude、GPT、Gemini 和 Kimi 模型，不需要第二个账户。" },
      ] },
      { h2: "50% 折扣从何而来", blocks: [
        { type: "p", text: "网关批量采购算力，再从预付的池化余额里卖出。你的每次调用先折算成官方 Anthropic 花费——输入、输出、缓存读取和写入分别计量——然后减去 B2C 统一 50% 折扣，只有净额会动你的余额。余额永不过期，也没有面向客户的订阅，闲置时间不花一分钱。" },
        { type: "p", text: "因为计量完全对齐官方价目表，常用的省钱手段依然有效：缓存读取远比新输入便宜，Haiku 每 token 的价格只是 Opus 的零头。折扣是在你提示词工程已经省下的基础上再叠加的。" },
        { type: "link", text: "充值前先估算一下负载", href: "/tools/claude-api-cost-calculator" },
        { type: "link", text: "各模型费率、上下文窗口和缓存定价", href: "/models" },
      ] },
      { h2: "直连没有的密钥级管控", blocks: [
        { type: "list", items: [
          "每个密钥可选的终身消费上限——硬性封顶，适合嵌进工具或发给团队的密钥。",
          "可选的到期日期，临时密钥到点自动失效，而不是永远躺在某人的配置里。",
          "控制台里逐请求的用量可见性，按模型和 token 类型拆分。",
          "即时开通，无需 Anthropic 账户、等待名单或账单国家限制——支持银行卡或加密货币支付。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户自带 $5 平台赠金余额，可用于支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码注册的账户不享受赠金。" },
      ] },
      { h2: "什么时候你不需要网关", blocks: [
        { type: "p", text: "这笔账要算得诚实。如果你已经有顺畅的 Anthropic 计费、企业协议，或者合规上要求必须直接和模型提供方签约，那就该直连——网关在你的请求路径上多加了一方，而这一方必须值得信任。如果你要的是同样的模型、官方花费减半、即时开通和预付的确定性，原生网关就是务实的选择。" },
      ] },
    ],
    faq: [
      { q: "Claude API 网关会改变 API 或模型吗？", a: "原生网关两者都不改。它讲标准的 Anthropic Messages API，提供相同的模型 ID，所以你的 SDK、流式、工具调用和提示词缓存的表现与直连 api.anthropic.com 完全一致。" },
      { q: "Claude API 网关和代理是一回事吗？", a: "不是。代理只转发流量，而网关理解协议，并额外提供认证、按 token 计量、计费和密钥管控，比如终身消费上限和到期日期。" },
      { q: "为什么用 Claude 网关而不是直连 Anthropic？", a: "为了官方花费基础上 B2C 统一 50% 的折扣、无需 Anthropic 账户和等待名单的即时开通、银行卡或加密货币支付，以及可选的按密钥管控。API 表面保持完全一致。" },
      { q: "一把网关密钥还能调 GPT、Gemini 和 Kimi 吗？", a: "可以。同一把 apiToken.sale 密钥覆盖支持的 Claude、GPT、Gemini 和 Kimi 模型——Anthropic Messages 用 x-api-key，OpenAI 兼容协议用 Authorization: Bearer，Gemini 原生协议用 x-goog-api-key。" },
      { q: "我现有的 Anthropic SDK 代码能通过网关工作吗？", a: "可以。把 SDK 的 base URL 设为 https://router.apitoken.sale，换成你的网关密钥，模型 ID 和消息代码保持不动。" },
    ],
  };
