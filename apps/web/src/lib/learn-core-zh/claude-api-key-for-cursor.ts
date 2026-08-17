import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "用于 Cursor 的 Claude API 密钥",
    h1: "在 Cursor 中使用 Claude API 密钥",
    description: "用 apitoken.sale 密钥把 Cursor 接入 Claude：将 Anthropic Base URL 设为 router.apitoken.sale，粘贴密钥，选择模型，即可以统一 50% 折扣编码。",
    keywords: ["用于 cursor 的 claude api 密钥", "cursor claude api", "cursor anthropic 密钥", "在 cursor 中用 claude", "不买 cursor pro 用 cursor"],
    dek: "Cursor 允许你自带 Anthropic 密钥，这意味着你可以用折扣预付余额在 Cursor 中运行 Claude，而不必依赖捆绑套餐。",
    sections: [
      { h2: "三步配置", blocks: [
        { type: "steps", items: [
          "打开 Cursor → Settings → Models → Anthropic API。",
          "把 Base URL 设为 https://router.apitoken.sale，并粘贴你的 sk-pool-••• 密钥。",
          "选择一个模型，例如 claude-opus-4-8，即可开始编码。",
        ] },
      ] },
      { h2: "配置", blocks: [
        { type: "code", code: `# Cursor → Settings → Models → Anthropic API\nBase URL : https://router.apitoken.sale\nAPI key  : sk-pool-•••\nModel    : claude-opus-4-8` },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "故障排查", blocks: [
        { type: "list", items: [
          "Cursor 忽略密钥：确认你编辑的是 Anthropic 提供方，而不是 OpenAI。",
          "找不到模型：设置一个当前的模型 ID，例如 claude-opus-4-8。",
          "401：重新检查 Base URL，并确认密钥完整粘贴。",
        ] },
        { type: "p", text: "连接成功后，所有受支持的 Claude 模型都可在同一把密钥和余额下使用。" },
      ] },
      { h2: "任何语言都能在 Cursor 中使用你的 Claude API 密钥", blocks: [
        { type: "p", text: "密钥与语言无关——无论是 Python、JavaScript、TypeScript、Go、Rust 还是其他项目，Cursor 都能在 Windows、macOS 和 Linux 上使用它。你配置的是模型提供方，而不是编程语言。" },
      ] },
    ],
    faq: [
      { q: "我能在 Cursor 里用自己的 Claude 密钥吗？", a: "可以。Cursor 的 Anthropic 提供方接受自定义 Base URL 和密钥，因此你可以把它指向 apitoken.sale。" },
      { q: "我还需要 Cursor Pro 吗？", a: "你可以用自己的 API 密钥和余额运行 Claude；而需要 Cursor 自身套餐的功能则与模型提供方无关，属于另一回事。" },
      { q: "Claude API 密钥能在 Windows 和 Mac 的 Cursor 里用吗？", a: "可以——Anthropic 提供方设置在 Windows、macOS 和 Linux 上完全相同。" },
    ],
  };
