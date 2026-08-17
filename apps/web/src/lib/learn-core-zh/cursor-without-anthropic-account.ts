import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "无需 Anthropic 账户在 Cursor 中用 Claude",
    h1: "无需 Anthropic 账户在 Cursor 中运行 Claude",
    description: "没有 Anthropic 账户？改用 apitoken.sale 密钥在 Cursor 中使用 Claude。即时开通，支持银行卡或加密货币支付，官方费率统一立省 50%。",
    keywords: ["无 anthropic 账户用 cursor", "cursor claude 无 anthropic", "cursor claude api 密钥", "不用 anthropic 账户用 claude"],
    dek: "如果你无法或不愿创建 Anthropic 账户，apitoken.sale 会签发自己的密钥，Cursor 会把它当作 Anthropic 提供方来接受。",
    sections: [
      { h2: "为什么可行", blocks: [
        { type: "p", text: "Cursor 与 Anthropic Messages API 通信。apitoken.sale 对外暴露的正是这套 API，因此 Cursor 分辨不出差别——它只是使用你的密钥和 Base URL。" },
      ] },
      { h2: "配置方法", blocks: [
        { type: "code", code: `# Cursor → Settings → Models → Anthropic API\nBase URL : https://router.apitoken.sale\nAPI key  : sk-pool-•••\nModel    : claude-opus-4-8` },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "你能保留什么", blocks: [
        { type: "list", items: [
          "完整的 Claude 系列——Opus、Sonnet 和 Haiku——都在一把密钥下。",
          "标准的 Anthropic 行为：流式输出、工具调用、系统提示。",
          "每把密钥可选终身累计消费上限和到期日期，并在控制台查看 token 级用量。",
        ] },
        { type: "p", text: "你使用 Cursor 的方式毫无变化；只是把密钥来源从 Anthropic 换成了 apitoken.sale。" },
      ] },
    ],
    faq: [
      { q: "这样做需要 Anthropic 账户吗？", a: "不需要。apitoken.sale 提供密钥和余额，因此无需 Anthropic 账户。" },
      { q: "这个集成用的是官方 Anthropic API 吗？", a: "Cursor 使用标准的 Anthropic Messages API；apitoken.sale 以折扣价提供同一套 API。" },
    ],
  };
