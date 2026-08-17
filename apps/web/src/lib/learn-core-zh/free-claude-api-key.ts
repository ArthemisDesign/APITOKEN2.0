import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "免费 Claude API 密钥助你上手",
    h1: "获取免费 Claude API 密钥开始使用",
    description: "通过 Google 或 GitHub 在 apitoken.sale 创建 Claude API 密钥，并获得 $5 平台欢迎奖励余额——无需银行卡或 Anthropic 账户。",
    keywords: ["免费 claude api 密钥", "claude api 免费", "claude api 免费额度", "免费 anthropic api 密钥", "claude api 免银行卡"],
    dek: "通过 Google 或 GitHub 创建账户，即可获得 $5 平台欢迎奖励余额并在充值前验证集成。邮箱密码账户不享受此奖励。",
    sections: [
      { h2: "“免费”包含什么", blocks: [
        { type: "list", items: [
          "一把可用于所有受支持 Claude 模型的 API 密钥。",
          "Google/GitHub 新账户可获一次性 $5 平台欢迎奖励余额，无需银行卡。",
          "足够的额度让你接通工具并跑通真实请求。",
        ] },
        { type: "p", text: "当你准备用更多时，充值任意整数美元金额，你的折扣就会自动生效。" },
      ] },
      { h2: "如何领取", blocks: [
        { type: "steps", items: [
          "通过 Google 或 GitHub 创建账户并打开控制台——无需审批、无需排队。",
          "生成一把 API 密钥（形如 sk-pool-…）。同一把密钥可用于所有受支持的 Claude 模型。",
          "将任意兼容 Anthropic 的工具指向 https://router.apitoken.sale，并携带 x-api-key 请求头向 /v1/messages 发送请求。",
        ] },
      ] },
      { h2: "Claude API 是永久免费的吗？", blocks: [
        { type: "p", text: "包含的 $5 平台欢迎奖励余额是免费起步额度，而不是无限的免费套餐。用完之后，你只为实际消耗的 token 付费——没有订阅、没有月度最低消费，预付余额也永不过期。" },
      ] },
    ],
    faq: [
      { q: "这些免费用量是真正的 API 访问吗？", a: "是的。Google/GitHub 账户的 $5 平台欢迎奖励余额可用于与付费余额相同的受支持模型和接口。" },
      { q: "开始使用需要银行卡吗？", a: "无需银行卡。通过 Google 或 GitHub 创建账户即可获得 $5 平台欢迎奖励余额。" },
      { q: "免费的 Claude API 密钥需要信用卡吗？", a: "不需要。通过 Google 或 GitHub 创建账户，即可在没有银行卡的情况下获得 $5 平台欢迎奖励余额。" },
    ],
  };
