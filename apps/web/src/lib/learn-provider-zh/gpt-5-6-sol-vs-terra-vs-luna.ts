import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "GPT-5.6 Sol、Terra 与 Luna 对比",
    h1: "GPT-5.6 Sol、Terra 与 Luna 对比",
    description: "从价格、推理强度、上下文和适用场景比较 GPT-5.6 Sol、Terra 与 Luna，为编程和生产任务选择合适模型。",
    keywords: ["gpt-5.6 sol 对比 terra", "gpt-5.6 terra 对比 luna", "最佳 gpt-5.6 模型", "gpt-5.6 模型", "gpt-5.6 对比", "编程 gpt 模型"],
    dek: "GPT-5.6 家族共享 400K 上下文、128K 最大输出和完整推理强度范围。实际差异在于每个 token 购买的能力与速度。",
    sections: [
      { h2: "按任务选择", blocks: [
        { type: "table", headers: ["层级", "适合场景", "官方输入 / 输出"], rows: [
          ["Sol", "高难推理、长周期代理、复杂代码审查", "$5 / $30"],
          ["Terra", "日常编程、生产对话、均衡代理", "$2 / $12"],
          ["Luna", "分类、抽取、路由和大批量简单任务", "$0.20 / $1.20"],
        ] },
        { type: "p", text: "Terra 是稳妥默认项：保留 Sol 的控制能力和上下文，token 价格仅 40%。评测显示质量不足时升级 Sol，确定性批量任务交给 Luna。" },
      ] },
      { h2: "三者共同点", blocks: [
        { type: "list", items: [
          "400K 上下文，最大输出 128K。",
          "文本和图像输入，文本输出。",
          "Responses 与 Chat Completions 均支持 SSE。",
          "GPT-5.6 家族支持从 none 到 max 的推理强度。",
          "同一端点、密钥和余额可按任务切换模型。",
        ] },
      ] },
    ],
    faq: [
      { q: "哪款 GPT-5.6 最适合编程？", a: "日常编程从 Terra 开始；最难的架构和代理任务用 Sol，便宜的确定性子任务用 Luna。" },
      { q: "Sol、Terra、Luna 需要不同端点吗？", a: "不需要。三者共用 OpenAI 兼容 base URL 和密钥，只修改 model ID。" },
      { q: "Terra 支持 max 推理强度吗？", a: "支持。Sol、Terra 与 Luna 使用同一套 GPT-5.6 推理强度，包括 max。" },
    ],
  };
