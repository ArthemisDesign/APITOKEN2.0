import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "最适合编程的 Claude 模型",
    h1: "最适合编程的 Claude 模型",
    description: "编程该用哪个 Claude 模型？一份按任务挑选 Opus、Sonnet 或 Haiku 的实用指南——所有型号都在一把 apiToken.sale 密钥上。",
    keywords: ["最适合编程的 claude 模型", "claude 编程模型", "opus 和 sonnet 编程对比", "claude 写代码用哪个", "哪个 claude 适合写代码"],
    dek: "最佳模型取决于任务。让模型匹配任务，就能用更少的 token 得到更好的产出——而且每一档模型都在同一把密钥上。",
    sections: [
      { h2: "日常编程用 Sonnet", blocks: [
        { type: "p", text: "Claude Sonnet 5 和 Sonnet 4.6 是交互式编码和智能体循环的默认之选：快速、能干且高性价比。大多数工作从这里开始。" },
      ] },
      { h2: "高难度问题用 Opus", blocks: [
        { type: "p", text: "在复杂重构、架构设计以及需要额外推理才划算的漫长高风险会话中，使用 Claude Opus 4.8。" },
      ] },
      { h2: "大批量用 Haiku", blocks: [
        { type: "p", text: "Claude Haiku 4.5 擅长快速、廉价、大批量的任务——代码检查、信息抽取、快速编辑——帮你撑长余额。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "最适合编程的 Claude 模型是哪个？", a: "日常编码用 Sonnet，复杂推理和重构用 Opus，快速大批量任务用 Haiku——全部在一把 apiToken.sale 密钥上。" },
      { q: "能按请求切换模型吗？", a: "能。一把密钥和余额覆盖所有模型，你可以把每个请求路由到性价比最高的那一档。" },
    ],
  };
