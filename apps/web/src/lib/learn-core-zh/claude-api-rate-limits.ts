import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude API 速率限制",
    h1: "理解 Claude API 速率限制",
    description: "apiToken.sale 上的 429 意味着什么、如何通过 Retry-After 与退避处理，以及密钥消费护栏与吞吐限制有何不同。",
    keywords: ["claude api 速率限制", "claude api 429", "anthropic 限流", "claude api 吞吐", "claude api 重试"],
    dek: "速率限制让网关保持稳定、让你的余额更安全。妥善处理它意味着工具更顺滑、不浪费开销。",
    sections: [
      { h2: "流量限制与消费护栏", blocks: [
        { type: "p", text: "apiToken.sale 不公布固定的 RPM 表。429 可能表示网关或上游容量限制。控制台不能配置请求吞吐；可用的按密钥护栏是可选的终身累计消费上限和到期日期。" },
      ] },
      { h2: "处理 429", blocks: [
        { type: "list", items: [
          "遵守 Retry-After 响应头并采用指数退避。",
          "降低并发，而不是猛冲端点。",
          "若需持续更高的吞吐，请联系支持。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "Claude API 的速率限制是多少？", a: "apiToken.sale 不公布固定的 RPM 数字。遇到 429 时请遵守 Retry-After、进行退避并降低并发；如需持续更高的吞吐，请联系支持。" },
      { q: "遇到 429 该怎么办？", a: "遵守 Retry-After、进行退避并降低并发；如需持续更高的限额请联系支持。" },
    ],
  };
