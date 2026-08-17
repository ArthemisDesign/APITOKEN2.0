import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude 3.5 与 Claude 4 对比——有何变化",
    h1: "Claude 3.5 与 Claude 4：有何变化",
    description: "从 Claude 3.5 迁移到当前的 Claude 4 系列？看看有哪些提升、更新后的模型 ID，以及如何在 apiToken.sale 上只改一处 base URL 就完成切换。",
    keywords: ["claude 3.5 对比 4", "claude 4 对比 3.5", "claude 模型迁移", "升级 claude 模型", "新版 claude 模型"],
    dek: "当前的 Claude 系列在推理和编码上相比 3.5 有明显提升。迁移基本上就是换一个模型 ID——其余一切照旧。",
    sections: [
      { h2: "有哪些提升", blocks: [
        { type: "p", text: "Opus、Sonnet 和 Haiku 4 系列模型在智能体编码、长上下文一致性和复杂推理方面相比 3.5 有所改进，同时沿用同一套 Messages API。" },
      ] },
      { h2: "如何迁移", blocks: [
        { type: "p", text: "把模型 ID 换成当前的某一个——例如 claude-opus-4-8、claude-sonnet-5 或 claude-haiku-4-5——并保留你现有的请求代码。在 apiToken.sale 上，密钥和端点都不变。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "Claude 4 比 3.5 强很多吗？", a: "是的，尤其在编码、智能体和长上下文任务上，同时使用相同的 API 格式。" },
      { q: "迁移难吗？", a: "不难——更新模型 ID（例如换成 claude-sonnet-5），你现有的 Messages API 代码即可继续工作。" },
    ],
  };
