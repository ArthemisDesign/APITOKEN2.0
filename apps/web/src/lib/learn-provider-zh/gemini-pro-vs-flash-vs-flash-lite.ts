import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Gemini Pro、Flash 与 Flash-Lite 对比",
    h1: "Gemini Pro、Flash 与 Flash-Lite 对比",
    description: "从价格、上下文、推理与适用场景比较 Gemini Pro、Flash 和 Flash-Lite，为编程、代理和大规模 API 选择模型。",
    keywords: ["gemini pro 对比 flash", "gemini flash 对比 flash lite", "最佳 gemini 模型", "gemini 模型对比", "编程 gemini 模型", "gemini 3.6 flash"],
    dek: "将模型层级作为路由选择：Pro 处理最难推理，Flash 作为编程默认，Flash-Lite 处理便宜的大规模步骤。一个密钥即可使用三者。",
    sections: [
      { h2: "按任务选择", blocks: [
        { type: "table", headers: ["层级", "适合场景", "推荐当前 ID"], rows: [
          ["Pro", "高难推理、规划、深度代码库和文档分析", "gemini-3.1-pro-preview"],
          ["Flash", "日常编程、多模态代理、均衡生产流量", "gemini-3.6-flash"],
          ["Flash-Lite", "分类、抽取、路由和便宜预处理", "gemini-3.1-flash-lite"],
          ["Image", "图像生成与编辑", "gemini-3.1-flash-image"],
        ] },
        { type: "p", text: "Gemini 3.6 Flash 是多数新文本任务的最佳起点。仅把最难请求升级到 Pro，把确定性批量任务降到 Flash-Lite。" },
      ] },
      { h2: "上下文与成本取舍", blocks: [
        { type: "list", items: [
          "当前文本模型提供 1M 上下文和最多 64K 输出。",
          "Pro 在 200K 输入后有长上下文溢价；Flash 与 Flash-Lite 在窗口内保持固定费率。",
          "文本模型缓存输入通常是新输入价格的 10%。",
          "大请求前使用 countTokens，并依据实际评测而非模型名称路由。",
        ] },
      ] },
    ],
    faq: [
      { q: "哪款 Gemini 最适合编程？", a: "从 Gemini 3.6 Flash 开始。复杂架构和审查升级到 3.1 Pro Preview，便宜的确定性步骤用 Flash-Lite。" },
      { q: "Flash-Lite 上下文更小吗？", a: "不是。已发布文本 Flash-Lite 保留 1M 上下文，优势是简单任务上的成本和延迟。" },
      { q: "切换层级需要新密钥吗？", a: "不需要。保持同一 Gemini base URL 与 x-goog-api-key，只修改 model ID。" },
    ],
  };
