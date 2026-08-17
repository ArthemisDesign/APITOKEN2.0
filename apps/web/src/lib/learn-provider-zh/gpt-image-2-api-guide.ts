import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "GPT Image 2 API 指南",
    h1: "用 GPT Image 2 API 生成和编辑图像",
    description: "通过 apiToken.sale 调用 GPT Image 2 生成和编辑图像：准确端点、model ID、参考图数量上限、token 计费方式和固定五折。",
    keywords: ["gpt image 2 api", "gpt-image-2", "gpt image 2 api 指南", "openai 图像生成 api", "gpt 图像编辑 api", "gpt image 2 价格", "images generations 接口", "openai images edits api", "gpt-image-2 model id", "图像生成 api 预付费"],
    dek: "apiToken.sale 上的 GPT Image 2 API 只有两条路由——POST /v1/images/generations 生成新图，POST /v1/images/edits 基于参考图做编辑——它们和 GPT 文本调用共用同一把 Bearer 密钥、同一个预付余额。用量按 token 计费，价格是 OpenAI 官方的一半。本文给出准确的请求写法、SDK 接入方式、真实价格表，以及上线前值得知道的接口限制。",
    sections: [
      { h2: "一个请求看懂生成路由", blocks: [
        { type: "p", text: "GPT Image 2 是通过 OpenAI 兼容接口调用的图像模型：向 /v1/images/generations 发送提示词，model 填 gpt-image-2，带上 Authorization: Bearer 请求头，就能拿回一张 PNG。不需要单独的图像套餐，也不需要第二把密钥——覆盖 GPT、Claude 和 Gemini 调用的同一把 sk-pool 密钥和预付余额，同样结算图像用量。" },
        sourceBlock("gpt-image-2-api-guide", 0, 1),
        { type: "p", text: "上面展示的三个控制字段就是该接口目前文档化的全部：background 只接受 opaque，quality 只接受 low，size 只接受 auto。超出这个组合的请求会被直接拒绝，而不是被静默近似——比如请求透明背景会返回错误，而不是返回一张被压平的 PNG。上线时把参数收敛在这个组合内，任何额外参数在实际调用验证通过之前都当作不支持。" },
        { type: "note", text: "当前接口每次调用返回一张非流式 PNG。不要围绕流式局部图像构建进度 UI；客户端超时按一次阻塞式请求渲染完整资产来设置。" },
      ] },
      { h2: "用最多五张 PNG 参考图编辑现有图像", blocks: [
        { type: "p", text: "编辑走的是另一条路由、另一种 content type。向 /v1/images/edits 发送 multipart/form-data，model 同样是 gpt-image-2，附上提示词和一到五张 PNG 参考图。参考图用来表达定点修改——重绘这张产品图的风格、换掉这个背景、扩展这条横幅——而不是从零重新生成。" },
        sourceBlock("gpt-image-2-api-guide", 1, 1),
        { type: "list", items: [
          "参考图必须是 PNG 文件；JPEG 或 WebP 资产先自行转换再上传，不要指望服务端代为处理。",
          "每次调用最多五张参考图——挑那几张真正承载修改意图的，而不是把整个素材库都传上去。",
          "每张参考图都按图像输入计费，所以同样的输出，编辑比纯提示词生成更贵。",
          "响应结构与生成一致：每次调用返回一张非流式 PNG。",
        ] },
        { type: "link", text: "更深入的编辑工作流：蒙版、批量与验收检查", href: "/docs/learn/image-editing-api-guide" },
      ] },
      { h2: "用官方 OpenAI SDK 调用", blocks: [
        { type: "p", text: "应用代码里不需要自己写 HTTP 层。官方 OpenAI SDK 已经暴露了 images API，把客户端切到 apiToken.sale 只需和文本模型相同的两个构造参数：base_url 和 api_key。" },
        sourceBlock("gpt-image-2-api-guide", 2, 1),
        { type: "p", text: "编辑场景下，同一个客户端暴露 images.edits，参考文件以二进制模式打开传入。密钥放在服务端环境变量里；图像端点和聊天端点同样敏感，因为它们扣的是同一个余额。" },
      ] },
      { h2: "一次生成的真实成本", blocks: [
        { type: "p", text: "不存在诚实的「每张图固定价」。GPT Image 2 按 token 计费，拆成四条计费腿——文本输入（你的提示词）、图像输入（编辑时的参考图）、缓存输入和图像输出——一次请求的总价以 API 返回的最终 usage 为准，与 PNG 的字节大小、尺寸无关。" },
        { type: "table", headers: ["计费项", "官方每 1M token", "本站价格"], rows: [
          ["新鲜文本输入", "$5", "$2.50"],
          ["新鲜图像输入", "$8", "$4"],
          ["缓存文本输入", "$1.25", "$0.625"],
          ["缓存图像输入", "$2", "$1"],
          ["图像输出", "$30", "$15"],
        ] },
        { type: "list", items: [
          "每条腿都享受统一的 50% B2C 折扣；缓存文本和图像输入在折扣生效之前，就先按普通输入费率的 25% 计费。",
          "读取每次响应里的 usage 对象，并把它和资产一起记录——它是计费权威，也是后台账单对账的依据。",
          "gpt-image-2 是不可变快照 gpt-image-2-2026-04-21 的别名，行为不会在调用之间漂移；想在代码里明确写出这个保证，就直接固定带日期的 ID。",
        ] },
        { type: "note", text: "不要拿几张测试渲染图就在自己的定价页面上标一个单图价格。输出用量随资产变化，三个样本推出来的数字到了生产环境一定是错的。用一周的真实 usage 把各条腿加起来，再做决定。" },
        { type: "link", text: "完整成本模型与节省测算", href: "/docs/learn/gpt-image-2-api-cost" },
      ] },
      { h2: "当前图像接口的限制", blocks: [
        { type: "p", text: "按这条路由今天可验证的行为做规划，而不是按营销名称想象。已确认的能力组合刻意很窄：" },
        { type: "list", items: [
          "每次调用一张 PNG，非流式——批量任务循环调用端点，而不是在一个请求里要 n 张图。",
          "控制项为 background opaque、quality low、size auto；其他取值（包括透明背景）都会被拒绝。",
          "编辑只接受 multipart/form-data 里的一到五张 PNG 参考图，其他一概不支持。",
          "图像用量和 GPT、Claude、Gemini 调用从同一个预付余额扣费——只需盯一个池子，而不是四个。",
        ] },
        { type: "p", text: "如果想换一个图像模型做对比，Gemini 侧的图像路由有并列文档，正面对比指南也覆盖了两者各自擅长的场景。" },
        { type: "link", text: "Nano Banana 2 与 GPT Image 2 同任务对比", href: "/docs/learn/nano-banana-2-vs-gpt-image-2" },
      ] },
      { h2: "在共享余额上控制图像开销", blocks: [
        { type: "p", text: "图像输出是最贵的一条腿，批量循环又会把它成倍放大，所以给图像 worker 单独发一把 API 密钥，并设置生命周期花费上限。失控的渲染任务会在自己的额度处停下，而不是抽干聊天流量依赖的余额；后台按密钥维度的用量统计也能准确告诉你哪个 worker 花了多少。" },
        { type: "steps", items: [
          "在后台为图像流水线创建一把专用密钥，把它的生命周期花费上限设为批量预算。",
          "发送一个有边界的生成请求（上面的 curl），确认返回的 PNG，以及包含预期计费腿的 usage 对象。",
          "用小规模循环跑你的真实提示词集，记录每个资产的最终 usage，并把总数与后台账单对账。",
          "确认无误后再放到完整批量规模，同时让密钥上限始终对准你实际批准的预算。",
        ] },
        { type: "link", text: "所有受支持提供商的模型费率", href: "/models" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账号可获得 $5 平台赠金——可用于支持的 Claude、GPT、Gemini 和 Kimi 模型；使用邮箱和密码注册的账号不享受赠金。" },
      ] },
    ],
    faq: [
      { q: "GPT Image 2 API 用哪个端点？", a: "生成新图用 POST /v1/images/generations，参考图编辑用 POST /v1/images/edits，两者都在 OpenAI 兼容基础 URL https://router.apitoken.sale/v1 上，带 Authorization: Bearer 请求头。" },
      { q: "GPT Image 2 能编辑现有图像吗？", a: "可以。edits 路由接受 multipart/form-data，包含一到五张 PNG 参考图和提示词，返回一张应用了所请求修改的 PNG。" },
      { q: "GPT Image 2 的准确 model ID 是什么？", a: "用 gpt-image-2，它是不可变快照 gpt-image-2-2026-04-21 的别名。想在代码里显式写明快照，就固定带日期的 ID。" },
      { q: "GPT Image 2 每张图多少钱？", a: "没有固定的单图价格：计费按最终 usage 计算，覆盖文本输入（官方 $5/M）、图像输入（$8/M）、缓存输入（新鲜输入的 25%）和图像输出（$30/M），本站每条计费腿统一五折——分别为每 1M $2.50、$4 和 $15。" },
      { q: "GPT Image 2 支持透明背景或流式输出吗？", a: "都不支持。已确认的能力组合是 background opaque、quality low、size auto，每次调用返回一张非流式 PNG；透明背景请求会被拒绝，而不是被近似处理。" },
      { q: "图像生成需要单独的密钥或余额吗？", a: "不需要。它和所有其他受支持模型（包括 GPT、Claude 和 Gemini）共用同一把 Bearer 密钥和预付余额——不过对批量图像 worker 来说，配一把带生命周期花费上限的专用密钥是合理做法。" },
    ],
  };
