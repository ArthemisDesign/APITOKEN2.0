import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Kimi K3 与 Kimi for Coding 对比",
    h1: "Kimi K3 与 Kimi for Coding 对比",
    description: "从上下文、推理控制、延迟和 token 价格比较 Kimi K3、K3 256K、Kimi for Coding 与 High Speed。",
    keywords: ["kimi k3 对比 kimi for coding", "kimi k3 api", "kimi k2.7 code", "最佳 kimi 编程模型", "kimi 模型对比", "kimi highspeed"],
    dek: "K3 面向推理与长上下文，Kimi for Coding 面向经济型编程。High Speed 用两倍费率换取延迟，K3 别名则选择 256K 或 1M 窗口。",
    sections: [
      { h2: "模型家族映射", blocks: [
        { type: "table", headers: ["公开 ID", "上下文", "适合场景"], rows: [
          ["kimi/kimi-for-coding", "256K", "日常编程与经济型代理循环"],
          ["kimi/kimi-for-coding-highspeed", "256K", "速度收益值得成本的低延迟编程"],
          ["kimi/k3-256k", "256K", "不需要完整窗口的 K3 推理"],
          ["kimi/k3 · kimi/k3[1m]", "1M", "大型代码库、文档与高难推理"],
        ] },
        { type: "p", text: "k3[1m] 是 K3 1M 模式的兼容写法，而不是独立模型。路由器会规范化为提供商实际接受的 k3。" },
      ] },
      { h2: "推理与路由", blocks: [
        { type: "list", items: [
          "K3 支持 low、high、max 推理强度，默认 high。",
          "Kimi for Coding 与 High Speed 始终启用 thinking。",
          "固定别名前先检查按密钥 /v1/models 目录。",
          "实用路由策略是日常代码用 Kimi for Coding，大型或困难工作升级到 K3。",
        ] },
      ] },
    ],
    faq: [
      { q: "哪款 Kimi 最适合编程？", a: "Kimi for Coding 是经济型默认。高难推理或长上下文选 K3，只有低延迟值得双倍费率时选 High Speed。" },
      { q: "k3 与 k3[1m] 是不同模型吗？", a: "不是。两者选择同一 K3 1M 模式，方括号形式是兼容别名。" },
      { q: "能直接请求内部官方模型 ID 吗？", a: "不能。请使用路由器目录返回的公开订阅别名，不要使用 kimi-k2.7-code 等费率 ID。" },
    ],
  };
