import type { LocalizedContent } from "../learn";
import { BASE, KEY } from "../learn-shared";

export const content: LocalizedContent = {
    title: "Cursor 配置 Claude API 密钥",
    h1: "在 Cursor 中使用 Claude API 密钥",
    description: "用 apitoken.sale 密钥把 Cursor 接入 Claude：将 Anthropic 提供方指向 router.apitoken.sale，粘贴密钥，选择模型，即可以官方价格统一 50% 折扣写代码。",
    keywords: ["cursor claude api 密钥", "cursor 配置 claude", "cursor anthropic api key", "cursor 自定义 base url", "cursor 自带 api 密钥", "cursor 不用 cursor pro", "claude api key for cursor", "claude api 密钥", "anthropic 兼容 api", "claude api base url"],
    dek: "给 Cursor 配一把 Claude API 密钥，就能用你自己的 Anthropic 兼容端点替代捆绑套餐，而在 Cursor 的设置里这只是两分钟的改动。把 Anthropic 提供方指向 apiToken.sale，同一份同时覆盖 GPT、Gemini 和 Kimi 的预付余额，就能驱动 Cursor 的聊天、内联编辑和智能体，价格统一为官方 token 价格的 50% 折扣。无需扩展，无需代理，无需排队。",
    sections: [
      { h2: "把 Cursor 的 Anthropic 提供方指向 router.apitoken.sale", blocks: [
        { type: "p", text: "Cursor 内置的 Anthropic 提供方支持自定义 Base URL 和 API 密钥，因此任何实现了 Anthropic Messages API 的端点都能驱动 Cursor 的聊天、Composer 和智能体。apiToken.sale 提供的正是这套 API，所以给 Cursor 配 Claude API 密钥只是一处设置改动：换端点、贴密钥、选模型。Cursor 发出的一切——系统提示词、工具定义、流式 token——都走标准协议，编辑器内的行为与 Anthropic 官方签发的密钥完全一致。" },
        { type: "steps", items: [
          "打开 apiToken.sale 控制台并生成一把密钥（形如 sk-pool-…）。一把密钥覆盖所有受支持的 Claude 模型，外加 GPT、Gemini 和 Kimi。",
          "在 Cursor 中进入 Settings → Models，滚动到 Anthropic 部分。它与 OpenAI 是两个独立的提供方——改错地方是最常见的配置失误。",
          `将 Anthropic 的 Base URL 设为 ${BASE}，粘贴你的 ${KEY} 密钥，然后让 Cursor 验证连接。`,
          "启用一个当前的模型 ID，例如 claude-opus-4-8——如果下拉列表里没有，就手动输入到模型列表中——然后在聊天模型选择器中选中它。",
        ] },
        { type: "code", code: `# Cursor → Settings → Models → Anthropic API\nBase URL : ${BASE}\nAPI key  : ${KEY}\nModel    : claude-opus-4-8` },
      ] },
      { h2: "先验证密钥，再排查 Cursor", blocks: [
        { type: "p", text: "当 Claude 在 Cursor 里没有响应时，问题要么出在密钥，要么出在编辑器——用三十秒判断清楚，而不是盲目地来回改设置。路由器暴露了 Anthropic Messages API，因此一条带 x-api-key 和 anthropic-version 请求头的 curl 就能走完 Cursor 将要使用的完整链路。如果返回了 JSON，说明你的密钥、余额和端点都正常，剩下的问题就在 Cursor 的设置里。" },
        { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-sonnet-5",\n    "max_tokens": 16,\n    "messages": [{"role": "user", "content": "ping"}]\n  }'` },
        { type: "p", text: "这里返回 401，说明密钥粘贴不完整或 Base URL 有拼写错误。返回模型未找到的错误，说明模型 ID 已过时——换成当前的 ID，例如 claude-sonnet-5 或 claude-opus-4-8。只有当 curl 成功而 Cursor 仍然失败时，才该怀疑编辑器：重新打开 Settings → Models，确认你改的是 Anthropic 提供方而不是 OpenAI，然后再点一次 Cursor 自带的验证按钮。" },
      ] },
      { h2: "按 Cursor 的实际用途匹配模型", blocks: [
        { type: "p", text: "Cursor 消耗 token 并不均匀。一次读取并改写十几个文件的 Composer 智能体任务，消耗可能比一次内联编辑高出几个数量级，所以选哪个模型取决于使用场景，而不是一个笼统的“最强模型”答案。由于所有受支持的 Claude 模型都在同一把密钥和同一份预付余额后面，切换模型只是点一下下拉框——把轻量任务交给便宜的模型，把高档模型留给真正需要的会话。" },
        { type: "table", headers: ["Cursor 场景", "推荐模型", "原因"], rows: [
          ["Agent / Composer 多文件编辑", "claude-opus-4-8", "推理最强；高难度重构中失败的编辑循环更少"],
          ["日常聊天和内联编辑", "claude-sonnet-5", "接近 Opus 的编码质量，token 单价低得多"],
          ["快速提问和小型补全", "claude-haiku-4-5", "最快也最便宜；适合随手一问"],
        ] },
        { type: "p", text: "用量按 token 从余额中计量扣减，因此一天以 Haiku 和 Sonnet 为主、偶尔升级到 Opus 的用法，花费远低于全程 Opus。控制台提供 token 级用量明细，你能看清哪个 Cursor 场景真正在花钱，并据此调整。" },
        { type: "link", text: "当前 Claude 模型 ID 与 token 单价", href: "/models" },
        { type: "link", text: "用成本计算器估算一个月的 Cursor 用量", href: "/tools/claude-api-cost-calculator" },
      ] },
      { h2: "预付余额、一把密钥、不依赖 Cursor Pro", blocks: [
        { type: "p", text: "这套方案刻意保持简单：用银行卡或加密货币给预付余额充值，每个请求按官方 token 价格减去统一 50% 折扣从余额扣费，没有任何按周期续费或过期的机制。没有席位费，没有捆绑包，也没有需要取消的订阅——余额用完，请求就会停止，直到你再次充值。那些要求 Cursor 自家付费套餐的功能与模型提供方相互独立：自带密钥改变的是 Claude 的计费方式，而不是 Cursor 提供的功能。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额，可用于受支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码账户不享受此奖励。" },
        { type: "p", text: "每把密钥都可以设置可选的终身消费上限和过期时间，因此为 Cursor 单独建一把密钥是限制编辑器花费的干净做法——一把给 Cursor，一把给脚本，在控制台里分开查看用量。密钥本身与语言和平台无关：无论 Python、TypeScript、Go 还是 Rust 项目，Cursor 用的是同一处设置，而 Anthropic 提供方的配置在 Windows、macOS 和 Linux 上完全一致。你配置的是模型端点，不是编程语言。" },
        { type: "p", text: "而且由于同一把密钥、同一份余额还能调用 GPT、Gemini 和 Kimi 模型，第二台机器、队友的编辑器或另一个工具都不需要新账号——只要把同一个 Base URL 和密钥粘贴到任何支持 Anthropic 兼容端点的客户端里即可。" },
      ] },
    ],
    faq: [
      { q: "我能在 Cursor 里用自己的 Claude API 密钥代替 Cursor Pro 吗？", a: "可以。Cursor 的 Anthropic 提供方接受自定义 Base URL 和密钥，你可以把它指向 apiToken.sale，用自己的预付余额运行 Claude。绑定 Cursor 自家套餐的功能与模型提供方相互独立。" },
      { q: "为什么 Cursor 提示我的 Claude API 密钥无效？", a: "几乎总是三个原因之一：你改的是 OpenAI 提供方而不是 Anthropic；Base URL 不完全是 https://router.apitoken.sale；或者密钥粘贴不完整。用你的 x-api-key 请求头向 /v1/messages 发一条简单的 curl，就能判断密钥本身是否正常。" },
      { q: "在 Cursor 里该选哪个 Claude 模型——Windows 和 Mac 上都能用吗？", a: "聊天和内联编辑默认选 claude-sonnet-5，长时间的 Composer 智能体会话选 claude-opus-4-8，快速廉价的小问题选 claude-haiku-4-5——全部在同一把密钥和同一份余额下。Anthropic 提供方的设置在 Windows、macOS 和 Linux 上完全相同。" },
    ],
  };
