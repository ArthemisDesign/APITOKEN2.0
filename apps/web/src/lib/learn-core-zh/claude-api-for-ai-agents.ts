import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "面向 AI 智能体的 Claude API",
    h1: "将 Claude API 用于 AI 智能体",
    description: "用 apitoken.sale 在 Claude API 上构建 AI 智能体：一把密钥通用所有模型，配合流式输出、工具调用、提示缓存和密钥终身累计消费上限，控制长时间运行的成本。",
    keywords: ["claude api 智能体", "claude ai 智能体 api", "claude 工具调用", "claude 智能体框架", "claude api 自动化"],
    dek: "智能体工作负载既耗 token 又长时间运行，这让模型选择、缓存和成本控制变得最为关键。以下是 apitoken.sale 如何契合智能体。",
    sections: [
      { h2: "智能体需要什么", blocks: [
        { type: "list", items: [
          "流式输出和工具调用——两者都是 Anthropic Messages API 的标准能力。",
          "模型路由：Haiku 处理廉价步骤，Sonnet 负责推理，Opus 应对最难的任务。",
          "为重复的系统提示和工具定义使用提示缓存。",
          "密钥终身累计消费上限，让失控循环无法花费超过该密钥的上限。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "一个成本感知的智能体循环", blocks: [
        { type: "p", text: "一个实用的模式：把规划和推理路由到 Sonnet，把廉价的子步骤和解析交给 Haiku，仅将最难的调用升级到 Opus。缓存系统提示和工具定义，让重复上下文几乎免费。" },
        { type: "list", items: [
          "设置密钥终身累计消费上限，让失控循环无法花费超过上限。",
          "使用流式输出，让智能体能够基于部分输出行动。",
          "关注 token 用量，以调优哪些步骤用哪个模型。",
        ] },
      ] },
    ],
    faq: [
      { q: "Claude API 适合做智能体吗？", a: "适合——具备流式输出、工具调用、模型路由和提示缓存，全都在一把 apitoken.sale 密钥下，并带消费管控。" },
      { q: "如何压低智能体成本？", a: "把廉价步骤路由到 Haiku，缓存重复上下文，并为智能体密钥设置终身累计消费上限。" },
    ],
  };
