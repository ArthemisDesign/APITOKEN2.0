import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "Gemini Pro vs Flash vs Flash-Lite：各档怎么选",
    h1: "Gemini Pro vs Flash vs Flash-Lite：按请求选对档位",
    description: "按真实 token 费率、上下文行为、缓存定价和负载匹配对比 Gemini Pro、Flash 与 Flash-Lite。一个 apiToken.sale 密钥即可路由全部三档——含图像输出。",
    keywords: ["gemini pro 和 flash 区别", "gemini flash 和 flash lite 区别", "gemini pro vs flash vs flash lite", "gemini 模型怎么选", "gemini 编程用哪个模型", "gemini 模型对比", "gemini 3.6 flash 和 3.1 pro", "gemini flash lite 使用场景", "gemini api 模型路由", "gemini 各档价格对比", "最便宜的 gemini 模型"],
    dek: "Gemini Pro、Flash、Flash-Lite 的取舍本质是路由问题，不是站队问题。Gemini 3.6 Flash 是编程和代理的默认档，Gemini 3.1 Pro Preview 是应对高难度推理的升级档，Gemini 3.1 Flash-Lite 承接便宜的批量步骤——三档共用一个密钥、一个端点和一个预付费余额。",
    sections: [
      { h2: "简短结论：默认 Flash，有据才上 Pro，走量用 Flash-Lite", blocks: [
        { type: "p", text: "把 Gemini 3.6 Flash 当作默认档；任务确实需要更深推理时再升级到 Gemini 3.1 Pro Preview；确定性的批量工作下放给 Gemini 3.1 Flash-Lite。三个文本档都是同样的 1M token 上下文、同样的 64K 输出上限、同样的 generateContent 请求结构，所以换档只改请求里的一个字段——永远不需要重新集成。" },
        { type: "p", text: "代价最高的错误出在两个极端。所有请求都跑 Pro，等于用 Pro 的输出费率为 Flash 同样能完成的工作买单；永远只用 Flash-Lite，则会在它本来解决不了的任务上陷入重试循环。把三档当成同一套系统的三种计价表：Flash 起草，Pro 处理例外，Flash-Lite 在两者前面做机械性的预处理。" },
      ] },
      { h2: "费率表：每 100 万 token 各档实际多少钱", blocks: [
        { type: "p", text: "价格是三档差异最大的地方。以下数字均为每 100 万 token，按输入 / 缓存输入 / 输出列出，同时给出 Google 官方价和 apiToken.sale 上固定 50% B2C 折扣后的实际价格——折扣对所有档位一视同仁。" },
        { type: "table", headers: ["档位", "Model ID", "官方 输入 / 缓存 / 输出", "五折后价格"], rows: [
          ["Pro", "gemini-3.1-pro-preview", "$2 / $0.20 / $12", "$1 / $0.10 / $6"],
          ["Flash", "gemini-3.6-flash", "$1.50 / $0.15 / $7.50", "$0.75 / $0.075 / $3.75"],
          ["Flash-Lite", "gemini-3.1-flash-lite", "$0.25 / $0.025 / $1.50", "$0.125 / $0.0125 / $0.75"],
          ["Flash-Lite (2.5)", "gemini-2.5-flash-lite", "$0.10 / $0.01 / $0.40", "$0.05 / $0.005 / $0.20"],
        ] },
        { type: "p", text: "两点最醒目。每一档贵的都是输出——是输入费率的四到八倍——所以一次通过、输出费率更低的模型，通常比需要反复重试的更强模型更划算。而且差距极大：Flash-Lite 的输出计费只有 Pro 的八分之一，这就是为什么把分类、抽取类工作从高档位路由出去，比任何提示词优化都更能省钱。" },
        { type: "note", text: "缓存输入是提供商单列的 usage 计费项，文本模型按新输入费率的 10% 计费——不会和同一批 token 的新输入叠加。在代理循环里复用长系统提示词和 few-shot 示例块，节省会快速累积。" },
      ] },
      { h2: "上下文窗口、输出上限与 200K 阈值", blocks: [
        { type: "p", text: "上下文基本不构成差异：当前的 Pro、Flash、Flash-Lite 文本模型都提供 1M token 窗口和最高 64K 输出。Flash-Lite 不是小上下文档——它的优势是简单任务上的成本和延迟，而不是更短的窗口。唯一会改变账单的上下文规则在 Pro 上。" },
        { type: "list", items: [
          "Gemini 3.1 Pro Preview 输入超过 200K token 时，整次请求按每 100 万 $4 输入、$18 输出重新计价——更高费率适用于全部 token，不只是超出部分。五折后为 $2/$9。",
          "Flash 和 Flash-Lite 在整个窗口内费率不变；一次 900K 输入的 Flash 调用和一次 1K 调用的单 token 价格完全相同。",
          "图像档是另一种形态：Gemini 3.1 Flash Image 提供 128K 上下文、最高 32K 输出，其缓存输入按完整输入费率计费，而不是文本模型的 10%。",
          "大调用之前，在同一模型路径上跑一次 countTokens——它是免费的，能在你付费之前告诉你这次请求是否越过 Pro 的 200K 阈值。",
        ] },
      ] },
      { h2: "哪类负载配哪一档", blocks: [
        { type: "p", text: "按失败成本而不是按感觉来匹配档位。当某一步答错或答得浅的代价低于升一档的 token 溢价时，这档就是对的。" },
        { type: "list", items: [
          "Pro（gemini-3.1-pro-preview）：跨文件重构、架构与设计权衡分析、深度文档审查，以及对 Flash 产出的最终审计——漏掉一个边界情况的代价高于 token 成本的工作。",
          "Flash（gemini-3.6-flash）：日常交互式编程、大量工具调用的代理循环、多模态输入，以及均衡的生产流量。凡是你没有实测过其他结论的场景，默认选它都对。",
          "Flash-Lite（gemini-3.1-flash-lite）：大规模的分类、抽取、路由、摘要等确定性预处理——请求可预测，质量门槛可以用程序验证。",
          "Image（gemini-3.1-flash-image）：任何必须包含渲染图像的响应。文本输出按每 100 万 $3 计费，图像输出按每 100 万 image token $60 计费（五折后分别为 $1.50 和 $30），所以纯文本任务绝不要用它。",
        ] },
        { type: "p", text: "较旧的 Gemini 2.5 Flash-Lite 仍在目录中，官方价 $0.10/$0.40——是已发布的最便宜文本档——对于已经在它上面验证过的高流量管道，它依然是合理选择。" },
      ] },
      { h2: "换档只改一个字段，密钥还是同一把", blocks: [
        { type: "p", text: "没有按档位划分的套餐、注册或端点。一个 apiToken.sale 密钥覆盖每一个 Gemini 档位——加上受支持的 Claude、GPT 和 Kimi 模型——共用同一个预付费余额。把原生 Gemini 协议指向 https://router.apitoken.sale，用 x-goog-api-key 头发送密钥，只改 model ID：" },
        sourceBlock("gemini-pro-vs-flash-vs-flash-lite", 4, 1),
        { type: "p", text: "把 gemini-3.6-flash 换成 gemini-3.1-pro-preview 或 gemini-3.1-flash-lite，同一个请求就跑在对应档位上。对同一 base URL 发起 GET /v1beta/models 会返回你的密钥可调用的准确 ID。各档位都通过 streamGenerateContent?alt=sse 支持流式，官方 Google GenAI SDK 除了 base URL 外无需任何改动。" },
        { type: "note", text: "SDK 的一个坑：base URL 只传裸主机名。Google SDK 会自己追加 /v1beta，base URL 如果以 /v1beta 结尾会出现重复路径，返回 404。" },
      ] },
      { h2: "一套让 Gemini 支出保持平稳的路由策略", blocks: [
        { type: "steps", items: [
          "所有负载默认走 gemini-3.6-flash——交互会话、CI 代理和生产流量都一样。",
          "预先定义升级触发条件：Flash 尝试失败或答得太浅、diff 跨的文件多到肉眼审不过来、或是不可逆的设计决策，就交给 gemini-3.1-pro-preview。",
          "把确定性子任务——意图分类、字段抽取、重排序——挪到 gemini-3.1-flash-lite，并用程序验证输出，让质量回退第一时间暴露。",
          "任何你怀疑接近 200K 输入的 Pro 请求，先调 countTokens；如果越线，要么裁剪上下文，要么有意识地接受 $4/$18 的长上下文费率。",
          "每周在看板里复核 token 级用量并调整分流比例——Flash-Lite 与 Pro 输出之间有八倍差距，路由上的一点小调整对账单的影响超过任何提示词微调。",
        ] },
        { type: "p", text: "由于 50% 折扣对各档位统一适用，相对排序永远不变——Flash-Lite 始终是最便宜的计价表，Pro 始终是高价档——所以按官方价格调好的路由策略在这里依然有效。" },
        { type: "link", text: "完整 Gemini 费率表，含图像输出和长上下文计费项", href: "/docs/learn/gemini-api-pricing" },
        { type: "link", text: "对比所有受支持模型与价格", href: "/models" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台奖励余额，适用于支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "编程该用哪个 Gemini 模型？", a: "从 Gemini 3.6 Flash 开始——它是交互式编程和代理循环在质量、速度和价格上的最佳平衡。高难度架构和审查工作升级到 Gemini 3.1 Pro Preview，便宜的确定性子任务用 Flash-Lite。" },
      { q: "Flash-Lite 的上下文窗口更小吗？", a: "不是。已发布的文本 Flash-Lite 模型保留与 Flash 和 Pro 相同的 1M token 上下文和 64K 输出上限。它的优势是简单任务上更低的成本和延迟，而不是更短的窗口。" },
      { q: "Gemini Pro 的长上下文定价什么时候生效？", a: "当 Gemini 3.1 Pro Preview 请求的输入超过 200K token 时，整次请求按官方每 100 万 $4 输入、$18 输出重新计价（五折后为 $2/$9）。Flash 和 Flash-Lite 没有长上下文溢价。不确定的话先跑一次免费的 countTokens。" },
      { q: "不换密钥能在 Pro、Flash、Flash-Lite 之间切换吗？", a: "可以。保持同一 base URL 和 x-goog-api-key 头，只改 generateContent 路径里的 model ID。一个密钥、一个预付费余额覆盖所有 Gemini 档位，以及受支持的 Claude、GPT 和 Kimi 模型。" },
      { q: "apiToken.sale 的折扣对三档都适用吗？", a: "适用。固定 50% B2C 折扣在精确计算官方计费项——输入、缓存输入、输出以及任何长上下文或图像计费项——之后应用，对 Pro、Flash、Flash-Lite 和 Flash Image 完全一致。" },
      { q: "高流量任务最便宜的 Gemini 模型是哪个？", a: "Gemini 2.5 Flash-Lite，官方价每 100 万 token $0.10/$0.40（五折后 $0.05/$0.20）；Gemini 3.1 Flash-Lite 官方价 $0.25/$1.50，是当前一代的平价档。" },
    ],
  };
