import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "在 Aider 中使用 Claude API",
    h1: "在 Aider 中使用 Claude API",
    description: "通过 apitoken.sale 在 Claude 上运行 Aider：导出 ANTHROPIC_API_BASE 和密钥，选一个 Claude 模型，以统一 50% 的折扣在终端结对编程。",
    keywords: ["claude api aider", "aider anthropic", "aider claude", "aider anthropic api base", "aider claude api 密钥"],
    dek: "Aider 是终端里的结对程序员，长会话烧 token 很快。用两个环境变量把它指向折扣网关，工作流保持原样。",
    sections: [
      { h2: "两个环境变量", blocks: [
        { type: "code", code: `export ANTHROPIC_API_KEY=sk-pool-•••\nexport ANTHROPIC_API_BASE=https://router.apitoken.sale\n\naider --model anthropic/claude-opus-4-8` },
        { type: "p", text: "Aider 底层通过 LiteLLM 路由 Anthropic 流量，而 LiteLLM 会读取 ANTHROPIC_API_BASE——因此无需任何配置文件。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额，足够你接通工具并在充值前跑通真实调用。" },
      ] },
      { h2: "为 Aider 选择模型", blocks: [
        { type: "list", items: [
          "anthropic/claude-opus-4-8——最难的重构和长程智能体编辑。",
          "anthropic/claude-sonnet-5——日常默认；编码质量接近 Opus。",
          "anthropic/claude-haiku-4-5——快速修改和低成本实验。",
        ] },
        { type: "p", text: "长 Aider 会话正是 token 折扣不断累积的地方：仓库地图、diff 和多文件编辑全部按输入和输出计费。" },
      ] },
    ],
    faq: [
      { q: "Aider 支持自定义 Claude 端点吗？", a: "支持。Aider 对 Anthropic 模型使用 LiteLLM，而 LiteLLM 读取 ANTHROPIC_API_BASE 环境变量——把它设为 https://router.apitoken.sale，然后正常启动 Aider 即可。" },
      { q: "在 Aider 里哪个 Claude 模型最好？", a: "claude-sonnet-5 是大多数编码工作的最佳默认；最难的多文件任务切到 claude-opus-4-8。两者共用同一把密钥。" },
      { q: "长 Aider 会话能便宜多少？", a: "每个请求按官方 token 费率计费再减去你 50% 的统一折扣，直连要花 $10 的会话在这里只需 $5。" },
    ],
  };
