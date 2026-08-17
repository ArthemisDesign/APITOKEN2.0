import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude 场景下 apiToken.sale 与 LiteLLM 对比",
    h1: "apiToken.sale 与 LiteLLM 对比",
    description: "LiteLLM 是一个统一各模型 API 的自托管代理，但需要你自己充值的密钥。apiToken.sale 则是一个托管的折扣 Claude 端点，无需自行运维。",
    keywords: ["litellm 替代品", "apitoken 对比 litellm", "litellm claude", "自托管 claude 代理", "托管 claude api"],
    dek: "如果你想跨多个提供方自托管一个代理，LiteLLM 很棒。apiToken.sale 是相反的取舍：无需运维，而且 Claude 余额自带折扣。",
    sections: [
      { h2: "自托管 vs 托管", blocks: [
        { type: "list", items: [
          "LiteLLM：你自己运行和维护代理，并且仍要自行为每个提供方充值。",
          "apiToken.sale：完全托管的原生 Anthropic 端点，无需管理任何基础设施。",
          "apiToken.sale 对 Claude 消费提供 50% 的统一折扣，这是裸代理做不到的。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "分别适合什么时候用", blocks: [
        { type: "list", items: [
          "apiToken.sale——你想要一个托管、带折扣、无需运维的 Claude 端点。",
          "LiteLLM——你想自托管一个跨多个自付费提供方的统一代理。",
          "你甚至可以把 LiteLLM 放在 apiToken.sale 密钥前面，在底层保留折扣。",
        ] },
      ] },
    ],
    faq: [
      { q: "LiteLLM 会给 Claude 打折吗？", a: "不会。LiteLLM 路由到你自己充值的提供方；折扣来自 apiToken.sale 汇集的预付余额。" },
      { q: "用 apiToken.sale 需要自己托管东西吗？", a: "不需要——它是托管端点。你只需改一下 Base URL 和密钥。" },
    ],
  };
