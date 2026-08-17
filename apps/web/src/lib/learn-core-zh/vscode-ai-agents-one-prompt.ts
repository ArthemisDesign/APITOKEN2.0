import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "免费 VS Code AI 智能体：用 Claude 驱动",
    h1: "在 VS Code 中用 Claude 运行免费 AI 智能体",
    description: "用 apitoken.sale 的 Claude 密钥配置 Cline、Roo Code 等免费 VS Code 智能体——无需 Cursor Pro。一个端点接通所有 Claude 模型，统一比官方价低 50%。",
    keywords: ["免费 vscode ai 智能体", "cline roo code claude", "vscode claude 智能体", "cursor 免费替代品", "vscode 用 claude 不用 cursor", "vscode 智能体编程", "cline 自定义 base url", "roo code anthropic api key", "claude api 接入 vscode 智能体", "免费 ai 编程智能体 vscode"],
    dek: "一个免费的 VS Code AI 智能体只需要两样东西：Cline 或 Roo Code 之类的扩展，以及一把兼容 Anthropic 的 API 密钥。把扩展指向 apitoken.sale 网关，Claude 就会执行你一条提示词下达的任务，统一比官方价低 50%——完全不涉及 Cursor Pro 订阅。",
    sections: [
      { h2: "一条提示词的智能体到底需要什么", blocks: [
        { type: "p", text: "想在 VS Code 里输入一条提示词，然后看着智能体做规划、改文件、跑终端命令、循环执行直到任务完成，你只需要一个免费的智能体扩展和一把模型密钥——仅此而已。Cursor Pro 不是必需品：Cline、Roo Code 等开源智能体接受任何兼容 Anthropic 的端点，所以 Claude 可以在原版 VS Code 里跑在你自己的 API 余额上。扩展是免费的；唯一计费的部分是模型流量，按 token 计费。" },
        { type: "p", text: "这个区分直接关系到你的钱包。订阅制打包的是一份你可能永远用不完的固定月度额度；按 token 计费的密钥只收智能体实际烧掉的部分。用 apitoken.sale 的密钥，所有支持的 Claude 模型都挂在同一个 Base URL 下，共用一份预付费余额，统一比官方价低 50%。" },
      ] },
      { h2: "把 Cline 或 Roo Code 接入网关", blocks: [
        { type: "steps", items: [
          "从 VS Code Marketplace 安装 Cline 或 Roo Code——两者都免费且开源。",
          "打开扩展的 API 提供方设置，选择 Anthropic。",
          "把 Base URL 设为 https://router.apitoken.sale，粘贴你的 sk-pool-••• 密钥。",
          "选择 claude-sonnet-5 作为起步模型，把第一个真实任务交给智能体。",
        ] },
        { type: "code", code: `# Cline / Roo Code → API provider settings\nAPI Provider : Anthropic\nBase URL     : https://router.apitoken.sale\nAPI Key      : sk-pool-•••\nModel        : claude-sonnet-5` },
        { type: "p", text: "两个扩展说的都是标准的 Anthropic Messages API：流式响应、工具调用和系统提示词的表现与规范描述完全一致，智能体根本分不出网关和直连的区别。密钥创建后即刻生效——没有审批队列，也没有等待名单。" },
      ] },
      { h2: "按智能体步骤选模型", blocks: [
        { type: "p", text: "智能体循环不是单一类型的工作。读文件、做小修改是廉价的高频流量；理清一次跨模块重构则不是。因为一把密钥覆盖所有 Claude 模型，你可以按任务在扩展里直接切换模型，而不必在多个账户或计费档案之间折腾。" },
        { type: "table", headers: ["模型 ID", "适用场景", "官方输入 / 输出（$/1M）", "本站（−50%）"], rows: [
          ["claude-haiku-4-5", "快速编辑、查找、高频步骤", "$1 / $5", "$0.50 / $2.50"],
          ["claude-sonnet-5", "默认选择：日常编码和智能体循环", "$3 / $15", "$1.50 / $7.50"],
          ["claude-opus-4-8", "复杂重构、架构设计、长会话", "$5 / $25", "$2.50 / $12.50"],
        ] },
        { type: "p", text: "一个实用打法：平时让智能体跑 Sonnet 5，机械性的多文件杂活降到 Haiku 4.5，只有任务真正需要深度推理时才升到 Opus 4.8。控制台按调用展示 token 级用量，每次会话具体花了多少一目了然。" },
        { type: "link", text: "查看完整 Claude 模型阵容与定价", href: "/models" },
      ] },
      { h2: "为什么智能体循环最能放大折扣", blocks: [
        { type: "p", text: "在智能体扩展里，一条提示词会变成很多次模型调用：智能体反复读你的文件、规划、编辑、跑测试、再检查自己的输出。一个感觉上只有一次交互的任务，很容易串起几十次请求。按 token 计费意味着成本随循环规模伸缩——而每个 token 上的统一折扣也随之放大。" },
        { type: "list", items: [
          "提示词缓存按官方缓存价再减去你的折扣计费，所以智能体每一步重读的长上下文都很便宜。",
          "输出 token 在智能体会话中占大头——每一次编辑、diff 和解释都是输出——每百万 token 的节省主要就集中在这里。",
          "没有席位费，也没有月度额度：闲置一周零成本，重度重构的周末也只付实际消耗的 token。",
        ] },
        { type: "link", text: "用 Claude API 成本计算器估算一次会话", href: "/tools/claude-api-cost-calculator" },
      ] },
      { h2: "一把密钥，通吃你所有的智能体", blocks: [
        { type: "p", text: "同一把密钥不绑定某一个扩展。它在 Cline、Roo Code、Continue、Cursor、Claude Code 和 Anthropic SDK 中同时可用，共用同一份余额——你可以让一个自主智能体在一个窗口里跑任务，同时在另一个窗口用轻量聊天扩展回答问题。除了 Claude，同一把密钥还能通过各自的协议访问支持的 GPT、Gemini 和 Kimi 模型，多模型工作流全部走同一份永不过期的预付费余额。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台奖励余额，可用于支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "第一条提示词失败？检查这三处", blocks: [
        { type: "list", items: [
          "401 Unauthorized：API 密钥或 Base URL 有误——两个都重新粘贴，注意不要带末尾空格。",
          "Model not found：扩展发送的是过时的模型 ID；换成 claude-sonnet-5 或 claude-opus-4-8 等当前 ID。",
          "响应慢或 429 错误：调低扩展的并发数，重试前遵守 Retry-After 响应头。",
        ] },
        { type: "note", text: "有些扩展即使你选了自定义提供方，仍会预填 Anthropic 的默认端点。如果请求还是打到 api.anthropic.com，到提供方设置里找单独的“使用自定义 Base URL”开关，并确认字段确实保存成功。" },
      ] },
    ],
    faq: [
      { q: "在 VS Code 里用 AI 智能体需要 Cursor Pro 吗？", a: "不需要。Cline、Roo Code 等免费开源扩展为原版 VS Code 带来智能体编程能力，接受任何兼容 Anthropic 的端点——用 apitoken.sale 密钥，唯一的成本是按 token 计的模型用量。" },
      { q: "如何把 Cline 或 Roo Code 指向 apitoken.sale？", a: "API 提供方选择 Anthropic，把 Base URL 设为 https://router.apitoken.sale，粘贴你的 sk-pool-… 密钥。同一套设置在两个扩展里都适用。" },
      { q: "VS Code 智能体该用哪个 Claude 模型？", a: "日常编码循环的默认选择是 claude-sonnet-5；复杂重构升到 claude-opus-4-8；廉价高频步骤降到 claude-haiku-4-5——全部用同一把密钥。" },
      { q: "这套配置跑一次 Claude 智能体会话要多少钱？", a: "按 token 计费，在 Anthropic 官方价基础上统一减 50%：Sonnet 5 折合每百万 token 输入 $1.50、输出 $7.50；提示词缓存按官方缓存价同样减去这个折扣。" },
      { q: "可以先不掏钱就试 VS Code 智能体吗？", a: "可以——通过 Google 或 GitHub 创建的账户自带 $5 平台奖励余额，足够在充值前跑真实的智能体任务。邮箱密码账户不享受此奖励。" },
      { q: "同一把密钥在 VS Code 之外也能用吗？", a: "可以。同一把密钥覆盖 Cursor、Claude Code 和 Anthropic SDK，还能用一份预付费余额访问支持的 GPT、Gemini 和 Kimi 模型。" },
    ],
  };
