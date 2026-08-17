import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "无需订阅即可使用 Claude Code",
    h1: "无需 $200/月套餐使用 Claude Code",
    description: "用即用即付的 API 余额运行 Claude Code，而非按月订阅。将 ANTHROPIC_BASE_URL 设为 router.apitoken.sale，只为实际使用量付费。",
    keywords: ["claude code 无需订阅", "claude code api 密钥", "claude code 即用即付", "claude code 便宜", "claude code 免订阅"],
    dek: "使用 Claude Code 不一定意味着固定月费套餐。把它指向一把带预付余额的 API 密钥，你就按 token 付费——如果你的用量起伏不定或只是想试试，这非常理想。",
    sections: [
      { h2: "两个环境变量", blocks: [
        { type: "code", code: `export ANTHROPIC_BASE_URL=https://router.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••\n\n# then just run\nclaude` },
        { type: "p", text: "全部改动就这些。Claude Code 保留每一项功能——它只是以折扣价从你的预付余额扣费，而非走订阅。" },
      ] },
      { h2: "即用即付何时更划算", blocks: [
        { type: "list", items: [
          "偶尔或突发式的用量，此时固定月费很浪费。",
          "在决定订阅套餐前先试用 Claude Code。",
          "让多个工具共用一份余额和一把密钥。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "Claude Code 能用自定义 API 密钥吗？", a: "可以。设置 ANTHROPIC_BASE_URL 和 ANTHROPIC_API_KEY，Claude Code 就会直接使用你的密钥和余额。" },
      { q: "我会失去任何功能吗？", a: "不会。Claude Code 表现完全一致；只是计费从订阅变为按 token 预付使用。" },
    ],
  };
