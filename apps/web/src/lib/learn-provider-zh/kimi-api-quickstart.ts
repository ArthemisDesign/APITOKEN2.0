import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "Kimi API 快速入门",
    h1: "使用 Anthropic SDK 快速接入 Kimi API",
    description: "通过 apiToken.sale 调用 Kimi K3 与 Kimi for Coding：Anthropic Messages、x-api-key、命名空间 model ID、终态 usage 和共享余额。",
    keywords: ["kimi api 快速入门", "kimi api 教程", "kimi anthropic api", "kimi k3 api 示例", "kimi for coding api", "kimi api curl"],
    dek: "Kimi 在统一路由器上使用 Anthropic Messages 协议。现有 Anthropic 客户端只需自定义 base URL、apiToken.sale 密钥与明确的 kimi/* model ID。",
    sections: [
      { h2: "使用 curl 发起首个请求", blocks: [
        sourceBlock("kimi-api-quickstart", 0, 0),
        { type: "p", text: "终态 usage 采用 Anthropic 结构，因此现有 usage 解析器可以继续使用。路由接受 stream: true，但提供商边界的增量性仍在进行实时验证。" },
      ] },
      { h2: "使用 Anthropic Python SDK", blocks: [
        sourceBlock("kimi-api-quickstart", 1, 0),
        { type: "note", text: "不要替换成 kimi-k2.7-code 等 Open Platform ID。公开路由器接受 GET /v1/models 返回的订阅别名；OpenAI 兼容客户端可通过统一 /v1 路由调用相同 Kimi 别名。" },
      ] },
    ],
    faq: [
      { q: "Anthropic SDK 能调用 Kimi 吗？", a: "可以。将 base_url 指向 https://router.apitoken.sale，并从按密钥目录中选择 kimi/* model ID。" },
      { q: "Kimi 路由可以设置 stream: true 吗？", a: "路由接受该参数，但上游和公共 chunk 的增量性仍在实时验证。如果 chunk 到达时序很重要，请使用非流式模式。" },
      { q: "应该从哪个 model ID 开始？", a: "编程默认选 kimi/kimi-for-coding；需要 K3 推理但不需要 1M 窗口时选 kimi/k3-256k。" },
    ],
  };
