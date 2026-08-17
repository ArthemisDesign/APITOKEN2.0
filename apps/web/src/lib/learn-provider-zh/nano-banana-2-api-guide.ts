import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "Nano Banana 2 API 指南",
    h1: "使用 Nano Banana 2 API 生成图像",
    description: "通过原生 Gemini API 调用 Gemini 3.1 Flash Image（Nano Banana 2）：准确的模型 ID、generateContent 请求、图像尺寸控制、分项定价和固定五折。",
    keywords: ["nano banana 2 api", "nano banana 2 api 密钥", "gemini 3.1 flash image api", "gemini 图像生成 api", "gemini-3.1-flash-image 模型 id", "nano banana 2 generatecontent", "nano banana 2 图像编辑 api", "gemini flash image 价格", "nano banana 2 图像尺寸", "google genai sdk 图像生成"],
    dek: "Nano Banana 2 是 Gemini 3.1 Flash Image 的公开名称，Nano Banana 2 API 就是原生 Gemini 的 generateContent 路由，只有一个准确的模型 ID：gemini-3.1-flash-image。它接受多模态输入，在同一响应中同时返回渲染好的图像部分和文本，并与 Gemini 文本调用共用同一个预付余额，按官方 token 费率五折结算。",
    sections: [
      { h2: "原生 Gemini 路由上的唯一模型 ID", blocks: [
        { type: "p", text: "Nano Banana 2 不是一个拥有独立端点的单独产品。它是 Gemini 3.1 Flash Image 的公开名称，通过原生 Gemini 的 generateContent 路由调用，模型 ID 准确写作 gemini-3.1-flash-image。你的 apiToken.sale 密钥放在 x-goog-api-key 请求头里，每次调用都从与 Gemini 文本模型相同的预付余额中结算。" },
        sourceBlock("nano-banana-2-api-guide", 0, 1),
        { type: "p", text: "响应是标准的 generateContent 载荷。按 MIME 类型解析 candidates[0].content.parts：文本部分承载模型的说明文字，图像部分以 base64 内联数据承载渲染好的资产。一个响应可以两者混合，所以永远不要假设第一个部分就是图像——遍历整个数组，按每个部分的 MIME 类型分支处理。" },
        { type: "note", text: "网关按模型 ID 路由，而不是按营销名称。请准确发送 gemini-3.1-flash-image；\"nano-banana-2\" 只是昵称，不是有效的模型 ID。" },
      ] },
      { h2: "在 generationConfig 中控制尺寸和宽高比", blocks: [
        { type: "p", text: "generationConfig 里的两个字段决定你要付多少钱、能交付什么。imageConfig.imageSize 选择输出档位——线上路由接受 1K、2K 和 4K——imageConfig.aspectRatio 固定画面比例。responseModalities 声明响应可以包含什么：想让说明文字和渲染图一起返回就传 [\"TEXT\",\"IMAGE\"]，只要像素就传 [\"IMAGE\"]。", },
        sourceBlock("nano-banana-2-api-guide", 1, 1),
        { type: "list", items: [
          "从 1K 开始，只有当资产达不到交付分辨率要求时才升到 2K 或 4K——图像输出是贵的部分。",
          "宽高比要显式传参，而不是在提示词里用文字描述；文字描述可能被宽松解读，参数不会。",
          "提示词保持具体：用一两句话写清主体、材质、光线、背景和构图，胜过一整段形容词。",
        ] },
      ] },
      { h2: "用多模态输入编辑现有图像", blocks: [
        { type: "p", text: "同一条路由既能生成也能编辑。把源图像作为 inline_data 部分放进 contents，带上它的 MIME 类型和 base64 数据，再加一个写指令的文本部分。模型会把编辑后的渲染图作为新的图像部分返回，所以生成和编辑共用同一条代码路径——只有请求的 contents 不同。" },
        sourceBlock("nano-banana-2-api-guide", 2, 1),
        { type: "p", text: "你附带的每张参考图都按输入 token 计费，所以参考集要保持精简。在所有候选方案之间复用同一套小而精选的参考图，而不是每次尝试都上传一大批。" },
      ] },
      { h2: "分项定价与模型限制", blocks: [
        { type: "p", text: "Flash Image 按 token 分项计量，和文本模型完全一样——图像部分只是更贵的第四项。apiToken.sale 在官方用量计算之后统一打五折，所以常规账户上每一项都按半价结算。" },
        { type: "table", headers: ["计费项", "官方每 1M token", "本站五折后"], rows: [
          ["文本输入", "$0.50", "$0.25"],
          ["文本输出", "$3", "$1.50"],
          ["图像输出", "每 1M 图像 token $60", "每 1M 图像 token $30"],
        ] },
        { type: "list", items: [
          "上下文窗口为 128K token，输出上限 32K——比文本 Flash 系列小，所以要精简过长的参考集和提示词。",
          "图像输出按图像 token 计量，而不是按文件计费；计费金额随你选择的尺寸档位缩放。",
          "这个图像模型的缓存输入没有折扣：按完整的 $0.50 官方输入费率计费。",
          "所有计费项都从与 Claude、GPT、Gemini 和 Kimi 调用相同的预付余额中扣除——没有单独的图像套餐。",
        ] },
        { type: "note", text: "只有当响应必须包含像素时，Flash Image 才值这个价。改写提示词、写图注或做分类，用同一把密钥调用文本 Flash 模型即可——它的输出项比图像项便宜一个数量级。" },
      ] },
      { h2: "用 Google GenAI SDK 生成", blocks: [
        { type: "p", text: "网关保留了原生 Gemini 协议，所以官方 Google GenAI SDK 只需改两处即可使用：你的密钥和 base URL。请求和响应结构与 Google 文档完全一致。" },
        sourceBlock("nano-banana-2-api-guide", 4, 1),
        { type: "note", text: "base_url 只填裸域名。SDK 会自行附加 /v1beta；重复的 /v1beta/v1beta 路径会返回 404。" },
      ] },
      { h2: "首次调用清单与预算护栏", blocks: [
        { type: "steps", items: [
          "创建免费账户并在仪表板生成密钥——它形如 sk-pool-…，覆盖所有支持的 Claude、GPT、Gemini 和 Kimi 模型。",
          "对 gemini-3.1-flash-image 跑一次免费的 countTokens 调用，在花钱买图像输出之前先估算输入项的开销。",
          "发送上面最小的 1K 请求，确认你能解码并保存返回的图像部分。",
          "打开仪表板核对这次调用：每个计费项的 token 用量、已应用的五折折扣和剩余余额在每次请求后都可见。",
        ] },
        { type: "p", text: "充值金额为整数美元，预付余额永不过期——生成时按 token 付费，不生成时一分钱不花。1K、2K 和 4K 图像输出项的逐尺寸成本拆解，见配套的成本指南。" },
        { type: "link", text: "Nano Banana 2 API 按图像尺寸的成本", href: "/docs/learn/nano-banana-2-api-cost" },
        { type: "link", text: "完整模型目录与逐模型定价", href: "/models" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户自带 $5 平台赠金，可用于支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱/密码注册的账户没有赠金。" },
      ] },
    ],
    faq: [
      { q: "Nano Banana 2 准确的 API 模型 ID 是什么？", a: "原生 Gemini generateContent 路由上的 gemini-3.1-flash-image。\"Nano Banana 2\" 是公开昵称，不能作为模型 ID。" },
      { q: "Nano Banana 2 的图像输出多少钱？", a: "官方价为每 1M 图像输出 token $60，apiToken.sale 固定五折后为 $30。文本输入和输出是独立的计费项，官方价 $0.50/$3，本站 $0.25/$1.50。" },
      { q: "Nano Banana 2 需要单独的图像 API 密钥吗？", a: "不需要。用同一把 sk-pool 密钥放在 x-goog-api-key 请求头里，与 Gemini 文本调用共用同一个预付余额。" },
      { q: "Nano Banana 2 能编辑现有图像吗？", a: "可以。把源图像作为 inline_data 部分（带 MIME 类型和 base64 数据）与文本指令一起放进 contents；编辑后的渲染图会作为新的图像部分返回。" },
      { q: "Gemini 3.1 Flash Image 的上下文和输出上限是多少？", a: "128K token 的上下文窗口，输出最多 32K token——比文本 Flash 系列小，所以提示词和参考集要保持精简。" },
      { q: "Nano Banana 2 的缓存输入更便宜吗？", a: "不。这个图像模型的缓存输入按完整的 $0.50 官方输入费率计费——预算里不要算缓存折扣。" },
    ],
  };
