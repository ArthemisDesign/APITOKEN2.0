import type { LocalizedContent } from "../learn";
import { KEY, OPENAI_BASE } from "../learn-shared";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "GPT-5.6 Sol vs Terra vs Luna 对比：该用哪档、什么时候用",
    h1: "GPT-5.6 Sol vs Terra vs Luna：按任务选档，而不是按习惯",
    description: "GPT-5.6 Sol、Terra、Luna 对比：官方价与五折价（每 1M token）、共享的 400K 上下文、推理强度，以及一套在单个 apiToken.sale 密钥上跑通三档的按请求路由策略。",
    keywords: ["gpt-5.6 sol 对比 terra", "gpt-5.6 terra 对比 luna", "gpt-5.6 sol vs terra vs luna", "gpt-5.6 编程用哪个模型", "gpt-5.6 模型对比", "gpt-5.6-sol 和 gpt-5.6-terra 区别", "gpt-5.6 价格档位", "编程选哪个 gpt 模型", "gpt-5.6 旗舰档和均衡档", "gpt-5.6 推理强度", "gpt 模型路由策略"],
    dek: "GPT-5.6 Sol、Terra、Luna 是同一模型家族的三个价位：同样 400K 上下文、128K 输出上限和推理控制，token 单价从 $0.20/$1.20 到 Sol 临时的 $4/$20（每 1M）。对大多数负载，正确答案是以 Terra 为默认档、Sol 作为升级档、Luna 承接大批量的机械性步骤——在 apiToken.sale 上，三档共用一把密钥、同一个预付余额，全线五折。",
    sections: [
      { h2: "简短答案：默认 Terra，Sol 和 Luna 守两端", blocks: [
        { type: "p", text: "几乎所有任务都用 gpt-5.6-terra，确实需要更深推理时再升级到 gpt-5.6-sol，把可预测的批量工作下放给 gpt-5.6-luna。Terra 保留了完整的 400K 上下文窗口、128K 输出上限和全部推理强度档位。按 Sol 临时费率，Terra 输入为 Sol 的 50%，输出为 60%，因此是编程、生产对话和 Agent 循环的正确默认档。" },
        { type: "p", text: "最贵的错误出在两个极端。所有请求都走 Sol，是在为本该由 Luna 完成的工作付旗舰价；死守 Luna 不放，又会在它本来就不可能完成的任务上白烧重试。把三档当成一个系统：Luna 承接简单的量，Terra 干真正的活，Sol 处理例外。" },
      ] },
      { h2: "一个家族，同一能力上的三种计价", blocks: [
        { type: "p", text: "Sol、Terra、Luna 不是三个不同的产品。它们共享 Responses 和 Chat Completions 接口、SSE 流式、文本加图像输入/文本输出，以及同一套推理强度——从 none 到 xhigh，GPT-5.6 全系还额外有 max。档位之间变化的只是能力深度、延迟和计费。以下价格均为每 1M token；折后那一列才是实际从预付余额里扣掉的钱。" },
        { type: "table", headers: ["", "gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"], rows: [
          ["官方输入 / 输出", "$4 / $20（临时）", "$2 / $12", "$0.20 / $1.20"],
          ["本站统一五折后", "$2 / $10", "$1 / $6", "$0.10 / $0.60"],
          ["缓存输入（官方）", "$0.40", "$0.20", "$0.02"],
          ["缓存写入（官方）", "$5", "$2.50", "$0.25"],
          ["上下文窗口", "400K tokens", "400K tokens", "400K tokens"],
          ["最大输出", "128K tokens", "128K tokens", "128K tokens"],
          ["推理强度", "none → max", "none → max", "none → max"],
          ["角色", "升级档", "日常主力", "走量档"],
        ] },
        { type: "note", text: "Sol 临时官方促销价有效至 2026-11-21（含当日）；自 2026-11-22 UTC 起恢复标准输入 $5、输出 $30。裸模型 ID gpt-5.6 是 gpt-5.6-sol 的别名，跟随相同费率。在生产配置里写死明确档位，别让沉默的默认值把日常流量悄悄路由到旗舰计价上。" },
      ] },
      { h2: "什么时候 Sol 值得比 Terra 多付费", blocks: [
        { type: "p", text: "Sol 是用来租的档位，不是用来住的。它的溢价买的是推理深度和长程一致性——把持住一个大 diff 或一个多步计划而不跑偏。升级的触发条件应该是证据，而不是感觉：一次失败的 Terra 尝试、涉及文件多到脑子装不下的重构，或一个你承担不起反悔成本的架构决策。" },
        { type: "list", items: [
          "多文件重构：漏掉一个边界情况的代价比 token 贵。",
          "疑难调试——竞态条件、内存损坏、没有明显原因的偶发失败测试。",
          "架构与设计权衡分析：一次错误判断远超任何 token 账单。",
          "Terra 生成的 diff 在合并前的最终审查。",
          "需要在数小时累积的上下文里保持连贯的长时自治 Agent 运行。",
        ] },
        { type: "p", text: "按促销价，Sol 输出 token 的价格是输入的五倍，所以最便宜的 Sol 调用是短调用。给它紧凑、边界清晰的提示词——失败的测试、相关的 diff、确切的问题——而不是未经筛选的代码库大杂烩。" },
      ] },
      { h2: "什么时候 Luna 在单位经济上赢过 Terra", blocks: [
        { type: "p", text: "按 Sol 临时费率，Luna 输入是 Sol 的 5%，输出是 6%，同时仍只有 Terra 的十分之一。因此任何它一次做对的任务几乎免费。但它的限制是真实存在的：吃深度的工作在 Luna 上会失败，最终还是得用 Terra 或 Sol 重试，省下的钱就被抹掉了。只把确定性、窄口径、易验证的工作路由给 Luna。" },
        { type: "list", items: [
          "生产流量中的分类、打标、路由和意图识别。",
          "抽取与格式化——JSON 整形、样板代码、重命名、一次性脚本。",
          "Agent 循环里的廉价子步骤：在结果交给主模型之前先摘要工具输出。",
          "对延迟敏感的回复：首 token 速度比最后一点质量分更重要。",
        ] },
        { type: "note", text: "先把分布量出来再定方案。如果 Luna 的输出里需要 Terra 重做的不止一小部分，这个「便宜档」的实际成本就是 Luna 加上重试——通常比直接把任务发给 Terra 更贵。" },
      ] },
      { h2: "切换档位只是改一个字段", blocks: [
        { type: "p", text: `没有按档位的注册、套餐或独立端点。一把 apiToken.sale 密钥（形如 ${KEY}）覆盖 Sol、Terra、Luna——以及支持的 Claude、Gemini、Kimi 模型——共用一个预付余额。档位间的路由就是在同一个 Responses 调用里换模型 ID：` },
        sourceBlock("gpt-5-6-sol-vs-terra-vs-luna", 4, 1),
        { type: "p", text: `把 "gpt-5.6-terra" 改成 "gpt-5.6-sol" 或 "gpt-5.6-luna"，同一个请求就跑在那一档——同样的 base URL、同样的 Bearer 头、同一个余额。用官方 SDK 时，路由策略就是一个构造函数加一行模型字符串：` },
        sourceBlock("gpt-5-6-sol-vs-terra-vs-luna", 4, 3),
        { type: "p", text: "控制台记录每笔请求结算后的 token 用量和确切的折后扣费，你能看到路由策略实际花了多少钱，而不是靠猜。随时可以用 GET " + OPENAI_BASE + "/models 确认当前启用的模型集——统一目录按提供商给 ID 划分命名空间（anthropic/*、openai/*、google/*）。" },
        { type: "link", text: "各模型完整规格与折后价格", href: "/models" },
      ] },
      { h2: "缓存和 272K 分界线的影响可能超过选档", blocks: [
        { type: "p", text: "有两个计费机制对账单的影响不亚于选对档位。第一是缓存输入：重复的提示词前缀按缓存费率计费——促销期 Sol 上每 1M $0.40，而新输入是 $4，Terra 和 Luna 同样是 10% 的比例；写入缓存则按普通输入的 125% 计费，促销期 Sol 为每 1M $5。在反复发送同一系统提示词和历史的长 Agent 循环里，稳定的前缀会累积成你能拿到的最大单笔节省。" },
        { type: "p", text: "第二是长上下文阶梯：输入超过 272K token 后，整个请求按 2 倍输入、1.5 倍输出重新计价——不只是超出部分。按 Sol 促销价，270K 输入加 2K 输出官方成本为 $1.12；273K 输入加 2K 输出为 $2.244。不管在哪一档，越线之前先把超大上下文拆开或裁剪历史。" },
        { type: "note", text: "推理 token 按输出 token 计费。在 Sol 促销期把强度拉满到 max，意味着为「认真思考」付每 1M $20（官方价）——当推理本身就是产出时值得，花在机械任务上就是浪费。让强度匹配档位：便宜档开高强度，往往不如强档开低强度划算。" },
      ] },
      { h2: "明天就能跑起来的路由策略", blocks: [
        { type: "steps", items: [
          "把所有负载默认到 gpt-5.6-terra——交互式编程、CI Agent 和生产流量一视同仁。",
          "提前写下升级触发条件：Terra 尝试失败、多文件重构、不可逆的设计决策，就带着边界紧凑的提示词去 gpt-5.6-sol。",
          "把确定性的高量步骤——分类、抽取、格式化——挪到 gpt-5.6-luna，并跟踪它的重做率，别让静默失败吃掉节省。",
          "保持提示词前缀稳定，让缓存输入费率生效；把请求控制在 272K 长上下文分界线以内。",
          "每周在控制台复盘按请求结算的用量，按实测成本调整分配，而不是按对模型名的感情。",
        ] },
        { type: "p", text: "统一的 50% B2C 折扣对三档一视同仁，所以相对排序永远不会变——Terra 的计费始终比 Sol 便宜，Luna 始终比 Terra 便宜。没有订阅费，也没有席位费：闲置的一周分文不花，重负载的一周也只按实际消耗的 token 付费，花的是官方价的一半。" },
        { type: "link", text: "GPT API 定价：账单每一条构成都讲清楚", href: "/docs/learn/gpt-api-pricing" },
        { type: "link", text: "OpenAI 兼容快速上手：从 curl 到官方 SDK", href: "/docs/learn/openai-api-quickstart" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户自带 $5 平台赠送余额——可用于支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码注册的账户不享受该奖励。" },
      ] },
    ],
    faq: [
      { q: "哪款 GPT-5.6 模型最适合编程？", a: "从 gpt-5.6-terra 开始：它保留 Sol 的 400K 上下文和完整推理控制，输入为 Sol 促销价的 50%，输出为 60%。最难的架构、调试或 Agent 任务升级到 gpt-5.6-sol，廉价的确定性子步骤用 gpt-5.6-luna。" },
      { q: "Terra 比 Sol 便宜多少？", a: "Sol 临时促销期间，Terra 官方输入/输出 $2/$12 分别是 Sol $4/$20 的 50%/60%。统一五折后 Terra 为 $1/$6，Sol 为 $2/$10。Sol 促销价有效至 2026-11-21（含当日）；自 2026-11-22 UTC 起恢复标准 $5/$30。" },
      { q: "Sol、Terra、Luna 用不同的端点或密钥吗？", a: "不用。三者跑在同一个 OpenAI 兼容 base URL 上，用同一把 Bearer 密钥、同一个预付余额；请求里只有模型 ID 不同。" },
      { q: "Terra 支持 max 推理强度吗？", a: "支持。Sol、Terra、Luna 暴露同一套 GPT-5.6 推理强度——none 到 xhigh 加 max。推理 token 按输出计费，所以 Sol 促销期 max 使用临时官方每 1M $20 的输出费率。" },
      { q: "gpt-5.6 和 gpt-5.6-sol 是同一个模型吗？", a: "gpt-5.6 是一个跟随旗舰的别名，因此按 Sol 费率计费。在生产配置中写死明确档位——gpt-5.6-sol、gpt-5.6-terra 或 gpt-5.6-luna——让计费可预测。" },
      { q: "输入超过 272K token 会怎样？", a: "OpenAI 长上下文费率对整个请求生效——2 倍输入、1.5 倍输出，在五折之前计算。不管哪一档，越线前先拆分或裁剪超大上下文。" },
    ],
  };
