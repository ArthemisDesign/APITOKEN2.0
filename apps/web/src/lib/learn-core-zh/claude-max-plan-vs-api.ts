import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude Max 订阅与 Claude API 对比",
    h1: "Claude Max 订阅与 API 对比",
    description: "何时该用 Claude 订阅、何时该用 Claude API。apiToken.sale 提供按量付费的全模型 API 权限，无月费，统一立省 50%。",
    keywords: ["claude max 订阅", "claude 订阅还是 api", "claude max 对比 api", "claude api 按量付费", "claude 免订阅"],
    dek: "固定的 Claude 订阅和按量付费的 API 计费适合不同的使用场景。对于程序化和突发式的使用，预付余额上的 API 通常更划算。",
    sections: [
      { h2: "订阅 vs 按 token 计费", blocks: [
        { type: "p", text: "对于单一应用内稳定、重度的交互式使用，固定月费套餐说得通。但对于突发式使用它就很浪费，而且它并不给你一把可编程、可接入自有工具的 API 密钥。" },
      ] },
      { h2: "为什么 API 往往更胜一筹", blocks: [
        { type: "list", items: [
          "只为实际用掉的 token 付费——没有月度保底。",
          "一把密钥驱动 Claude Code、Cursor、智能体和生产环境调用。",
          "apiToken.sale 在官方 token 费率上再享统一 50% 折扣。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "API 比 Claude 订阅更便宜吗？", a: "对于突发式或程序化的使用，按量付费的 API 计费能避免为闲置时间支付固定月费，而 apiToken.sale 还会进一步打折。" },
      { q: "能在编码工具里用 API 吗？", a: "能——API 密钥可用于 Claude Code、Cursor、VS Code 智能体和各 SDK，这些是订阅所不提供的。" },
    ],
  };
