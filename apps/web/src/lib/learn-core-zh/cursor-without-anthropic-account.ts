import type { LocalizedContent } from "../learn";
import { BASE, KEY } from "../learn-shared";

export const content: LocalizedContent = {
    title: "无需 Anthropic 账户，在 Cursor 中使用 Claude",
    h1: "无需 Anthropic 账户，在 Cursor 中运行 Claude",
    description: "没有 Anthropic 账户？改用 apiToken.sale 密钥在 Cursor 中使用 Claude。即时开通，支持银行卡或加密货币支付，官方 API 费率统一 5 折。",
    keywords: ["无 anthropic 账户用 cursor", "cursor 不用 anthropic 账户", "cursor anthropic api key", "cursor 自定义 anthropic base url", "cursor 自带 api 密钥", "cursor claude api 密钥", "在 cursor 中运行 claude", "cursor byok claude", "anthropic 兼容 api", "cursor 无订阅用 claude"],
    dek: "在没有 Anthropic 账户的情况下使用 Cursor，关键只有一个细节：Cursor 的 Anthropic 提供方接受任意兼容的 Base URL 和密钥，而 apiToken.sale 提供的正是这套 API。本文带你走完设置流程，说明 Cursor 里哪些功能真正跑在你的密钥上，并解释统一按官方 API 费率 5 折计费的预付模式如何取代 Anthropic 账单。",
    sections: [
      { h2: "可以——Cursor 只需要一个 Base URL 和一把密钥", blocks: [
        { type: "p", text: "在 Cursor 中运行 Claude 并不需要 Anthropic 账户。Cursor 的 Anthropic 提供方允许你覆盖 Base URL 并粘贴自己的 API 密钥，而 apiToken.sale 签发的密钥恰好能填进这个位置。注册、把两个值粘进设置，Claude 就会在 Cursor 里回答——整个过程完全不涉及 Anthropic。" },
        { type: "p", text: "这之所以可行，是因为 Cursor 走的是 Anthropic Messages API：向 /v1/messages 发起 POST 请求，带上 x-api-key 和 anthropic-version 请求头。apiToken.sale 的路由器暴露的正是这套 API，所以 Cursor 分辨不出差别——它发出的请求形状与发给 Anthropic 的完全相同，收到的响应形状也完全相同。流式输出、工具调用和系统提示词都遵循标准 Anthropic 行为，因为线上跑的就是标准 Anthropic 协议。" },
        { type: "p", text: "请求从本机直达你配置的端点。BYOK 流量不会经 Cursor 服务器中转，也没有额外环节需要排查：只要端点有响应，Cursor 就能用。" },
      ] },
      { h2: "把 Cursor 的 Anthropic 提供方指向 apiToken.sale", blocks: [
        { type: "steps", items: [
          "打开 Cursor → Settings → Models，滚动到 Anthropic API 部分。",
          `将 Base URL 设为 ${BASE}，把你的 apiToken.sale 密钥（形如 ${KEY}）粘贴到 API key 字段。`,
          "把一个当前模型 ID 加进模型列表——claude-opus-4-8 是稳妥的默认选择——并确认旁边的开关已打开。",
          "打开一个聊天窗口，选中该模型，随便发一条消息。收到流式回复就说明密钥、Base URL 和计费都已生效。",
        ] },
        { type: "code", code: `# Cursor → Settings → Models → Anthropic API\nBase URL : ${BASE}\nAPI key  : ${KEY}\nModel    : claude-opus-4-8\n\n# Optional: verify the endpoint before you even open Cursor\ncurl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{"model":"claude-opus-4-8","max_tokens":64,"messages":[{"role":"user","content":"ping"}]}'` },
        { type: "p", text: "花三十秒跑一次 curl 检查是值得的。如果返回了 JSON 补全，之后的一切问题都在 Cursor 设置里；如果返回鉴权错误，说明密钥本身有问题，在 Cursor 里怎么点都解决不了。" },
      ] },
      { h2: "Cursor 里哪些功能跑在你的密钥上，哪些不是", blocks: [
        { type: "p", text: "自带 Anthropic 密钥会改道所有调用 Claude 模型的功能。它不会改变 Cursor 自家功能的提供方式，也不会解锁 Cursor 仅对自家套餐开放的内容。" },
        { type: "list", items: [
          "聊天、Composer 和智能体模式都跑在你选定的 Claude 模型上，token 消耗从你的预付余额中扣除。",
          "内联编辑（Cmd/Ctrl+K）使用同一个选定模型和同一把密钥。",
          "Cursor Tab 自动补全由 Cursor 自家的补全模型提供，不走 Anthropic API——与你的密钥完全无关，Tab 是否可用仍取决于你的 Cursor 套餐。",
          "Cursor 保留给自家订阅用户的功能依然保留；模型提供方密钥改变的是模型调用的去向，而不是你的 Cursor 许可包含什么。",
        ] },
        { type: "note", text: "常见的误解：聊天里 Claude 能回答，但 Tab 建议没了。这是正常现象——Tab 从来没用过你的 Anthropic 密钥，即使密钥来自 Anthropic 官方也一样。这两套系统的计费和提供方都是独立的。" },
      ] },
      { h2: "一把密钥，覆盖整个 Claude 系列", blocks: [
        { type: "p", text: "一把 apiToken.sale 密钥即可解锁完整的 Claude 产品线——Opus、Sonnet 和 Haiku——你可以在 Cursor 内切换档位，不必折腾多份凭据。在 Settings → Models 中加入各个模型 ID，按任务选择：" },
        { type: "table", headers: ["模型 ID", "档位", "在 Cursor 中的适用场景"], rows: [
          ["claude-opus-4-8", "Opus", "智能体模式下的多文件重构和最烧脑的推理任务"],
          ["claude-sonnet-5", "Sonnet", "日常主力：聊天、内联编辑和大多数智能体运行"],
          ["claude-haiku-4-5", "Haiku", "快速低成本的迭代——重命名、小修小补、随手一问"],
        ] },
        { type: "p", text: "三个模型都从同一份余额扣费，实用策略是：默认用 Sonnet，一次性提示词降到 Haiku，把 Opus 留给答错代价比 token 更贵的任务。" },
        { type: "link", text: "查看当前模型阵容及各模型定价", href: "/models" },
      ] },
      { h2: "预付计费，取代 Anthropic 账单", blocks: [
        { type: "p", text: "既然另一端没有 Anthropic 账户，自然也没有 Anthropic 账单。你用银行卡或加密货币给 apiToken.sale 余额充值，Cursor 发出的每个请求按 token 从余额中扣费，统一为官方 API 费率的 5 折。即时开通：密钥生成即可用，没有排队名单，也没有用量分级审核。" },
        { type: "list", items: [
          "控制台提供每把密钥的 token 级用量，你能精确看到 Cursor 每天花了多少。",
          "每把密钥可选终身消费上限和到期日期——给 Cursor 单独签一把密钥并设好上限，失控的智能体循环最多也只能烧掉你允许的额度。",
          "同一份余额之后还能调用 GPT、Gemini 和 Kimi 模型，供其他工具使用；没有任何东西被锁定在 Cursor 或 Claude 上。",
        ] },
        { type: "link", text: "用免费计算器估算一个月的 Cursor 用量", href: "/tools/claude-api-cost-calculator" },
      ] },
      { h2: "从注册到第一个回答", blocks: [
        { type: "steps", items: [
          "创建 apiToken.sale 账户——用 Google 或 GitHub 注册可获得 $5 平台奖励余额（邮箱密码账户不享受此奖励）。",
          "余额不够时用银行卡或加密货币充值；奖励余额足够先把整套配置验证一遍。",
          "生成 API 密钥（sk-pool-…），如需硬性上限，可设置终身消费上限和到期日期。",
          "按上文所示把 Base URL 和密钥粘进 Cursor，选中 claude-opus-4-8，发出你的第一个提示词。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额，可用于受支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码账户不享受此奖励。" },
        { type: "p", text: "完成这些之后，你使用 Cursor 的方式不会有任何变化。只是把密钥和余额的来源从 Anthropic 换成了 apiToken.sale——而且自始至终都不需要创建 Anthropic 账户。" },
      ] },
      { h2: "大家实际会遇到的三个配置错误", blocks: [
        { type: "list", items: [
          "401 Unauthorized：密钥粘贴时被截断或混入了多余空格，或者你改的是 OpenAI 提供方而不是 Anthropic 提供方。在 Anthropic 部分重新粘贴完整密钥。",
          "Model not found：模型 ID 不在 Cursor 的模型列表里，或已过时。加入准确字符串 claude-opus-4-8 并启用它。",
          "验证按钮失败：Base URL 填错了。它必须是裸的路由器源站——不带 /v1 后缀，不带尾部路径——因为 Cursor 会自己拼接 Messages API 路径。",
        ] },
        { type: "note", text: "如果聊天正常，但在超长智能体运行中响应半路中断，先去控制台检查密钥的终身消费上限——额度耗尽或过期的密钥正是这种表现。" },
      ] },
    ],
    faq: [
      { q: "在 Cursor 中使用 Claude 需要 Anthropic 账户吗？", a: "不需要。apiToken.sale 提供密钥和预付余额，Cursor 会在其 Anthropic 提供方位置接受这把密钥——任何一步都不需要创建或持有 Anthropic 账户。" },
      { q: "这是官方 Anthropic API 吗？", a: "Cursor 使用标准的 Anthropic Messages API，apiToken.sale 在其路由器上提供同一套 API，价格统一为官方费率的 5 折。请求和响应形状、流式输出、工具调用和系统提示词均遵循标准行为。" },
      { q: "用自己的 Anthropic 密钥，Cursor Tab 自动补全还能用吗？", a: "Tab 由 Cursor 自家的补全模型提供，不走 Anthropic API，因此不受你粘贴哪把密钥影响——它是否可用取决于你的 Cursor 套餐，而不是 API 密钥。" },
      { q: "这套方案在 Cursor 里能用哪些 Claude 模型？", a: "一把密钥覆盖完整产品线：Opus、Sonnet 和 Haiku。在 Settings → Models 中加入 claude-opus-4-8 等模型 ID，按任务切换即可。" },
      { q: "没有 Anthropic 账户，用量怎么付费？", a: "用银行卡或加密货币给 apiToken.sale 预付余额充值，Cursor 用量按 token 扣费，统一为官方 API 费率的 5 折。通过 Google 或 GitHub 创建的新账户可获 $5 奖励余额。" },
      { q: "能限制 Cursor 密钥的消费额度吗？", a: "可以。每把密钥都可设置可选的终身消费上限和到期日期，控制台还提供每把密钥的 token 级用量，给 Cursor 配一把专用密钥很容易控制预算。" },
    ],
  };
