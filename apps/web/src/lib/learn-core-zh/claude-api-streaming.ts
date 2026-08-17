import type { LocalizedContent } from "../learn";
import { BASE, KEY } from "../learn-shared";

export const content: LocalizedContent = {
    title: "Claude API 流式输出（SSE）：token 边生成边到达",
    h1: "Claude API 流式输出：SSE 响应逐 token 推送",
    description: "Claude API 流式输出在 apiToken.sale 上如何工作：stream:true、Anthropic SSE 事件序列、SDK 辅助方法、最终 token 用量，以及为什么计费与非流式完全一致。",
    keywords: ["claude api 流式输出", "claude sse", "claude 流式响应", "anthropic streaming api", "claude api server-sent events", "claude messages api stream true", "anthropic sdk 流式", "claude api 实时响应", "claude streaming python", "claude api stream 示例"],
    dek: "Claude API 流式输出在 token 生成的同时就通过 server-sent events 逐个推送，而不是让你干等整条消息。在 apiToken.sale 上，它是同一端点上的标准 Anthropic SSE 格式，按 token 计费，与非流式调用完全一样。本文带你走一遍请求、事件序列，以及生产环境中真正重要的失败模式。",
    sections: [
      { h2: "用 stream:true 开启流式输出", blocks: [
        { type: "p", text: `Claude API 流式输出只是一个开关，不是一个新端点。向 ${BASE}/v1/messages 发 POST，在 x-api-key 头里带上你的密钥，加上 anthropic-version: 2023-06-01 头，body 里放 "stream": true——网关就会返回标准的 Anthropic server-sent events 流，而不是单个 JSON 文档。请求结构、模型 ID 和请求头与 api.anthropic.com 期望的完全一致，所以任何已经会说 Messages API 的客户端都能原封不动地流式。` },
        { type: "code", code: `curl -N ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-sonnet-5",\n    "max_tokens": 1024,\n    "stream": true,\n    "messages": [{"role":"user","content":"Hello"}]\n  }'` },
        { type: "p", text: "-N 参数会关掉 curl 的输出缓冲——这是第一次测流式时“看起来坏了”最常见的原因：响应其实到得好好的，但 curl 会一直攒到连接关闭才吐出来。关掉缓冲后，你能实时看着 text_delta 事件逐个落地。响应的 Content-Type 是 text/event-stream，连接会一直保持到模型生成结束。" },
        { type: "steps", items: [
          `在控制台生成一个密钥——形如 ${KEY}，对所有支持的 Claude 模型通用。`,
          "带 -N 运行上面的 curl，观察事件增量到达。",
          "确认流以 message_start 开始，中间是 content_block_delta 分块，最后以 message_delta、message_stop 收尾。",
          "打开控制台的用量视图，把这次请求的输入、输出 token 和流里上报的数字对一遍。",
        ] },
      ] },
      { h2: "读 SSE 事件序列，而不是原始文本", blocks: [
        { type: "p", text: "Anthropic 的流是一个带类型的事件序列；把它当原始文本流处理，正是手写客户端栽跟头的地方。每个事件由一行 event: 名称加一行 data: JSON 组成。一个短回答的最小流长这样：" },
        { type: "code", code: `event: message_start\ndata: {"type":"message_start","message":{"id":"msg_01...","role":"assistant","model":"claude-sonnet-5","usage":{"input_tokens":12,"output_tokens":1}}}\n\nevent: content_block_start\ndata: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}\n\nevent: content_block_delta\ndata: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}\n\nevent: content_block_stop\ndata: {"type":"content_block_stop","index":0}\n\nevent: message_delta\ndata: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":9}}\n\nevent: message_stop\ndata: {"type":"message_stop"}` },
        { type: "table", headers: ["事件", "携带的内容"], rows: [
          ["message_start", "消息外壳：id、role、model，以及提示词的 usage.input_tokens"],
          ["content_block_start / content_block_stop", "给定 index 上每个输出块——text 或 tool_use——的边界"],
          ["content_block_delta", "文本块的增量 text_delta；工具调用的 input_json_delta 片段"],
          ["ping", "块之间的保活信号；可以安全忽略，但不要当成错误"],
          ["message_delta", "stop_reason 和累计的 usage.output_tokens——权威的输出计数"],
          ["message_stop", "流的终点；连接在此之后关闭"],
        ] },
        { type: "p", text: "从这个序列可以得出两条记账规则。第一，input_tokens 在 message_start 时就是定值，而 output_tokens 是累计的，只有携带 stop_reason 的那条 message_delta 里才是最终值——所以要从终止事件里读 usage，绝不要自己数 delta。第二，一次生成可能包含多个内容块（text 与 tool_use 交错），各有自己的 index，所以要按 index 分别累计 delta，而不是把所有内容追加进同一个字符串。工具参数以 input_json_delta 的部分 JSON 片段到达，必须先拼接完整，再在 content_block_stop 时一次性解析。" },
        { type: "p", text: "不用 SDK 消费流只需要一条纪律：按协议边界切分，而不是按网络数据块切分。以流的方式读取响应体（在浏览器和 Node 18+ 中，res.body 是一个 ReadableStream），缓冲字节直到遇到空行，把两个空行之间的内容视为一个事件。网络分块和事件并不对齐——一行 data: 可能被拆到两次读取里，多个事件也可能挤在一次读取里。只从 data: 载荷解析 JSON，并且只解析你要处理的事件类型。EventSource 在这里不适用：它只会发 GET，而 Messages API 要求 POST。" },
        { type: "note", text: "长流里可能夹着 ping 事件，也可能在模型“思考”时暂时安静。读取超时要针对事件之间的静默时间设置，而不是针对整条流的总时长——一刀切的 30 秒总超时会杀掉正常的长生成。" },
      ] },
      { h2: "流式改变了什么——又没改变什么", blocks: [
        { type: "p", text: "官方 SDK 把事件管道都藏起来了。把客户端指向网关，用它的流式辅助方法即可：上面那些事件会以迭代器的形式暴露，带权威 usage 的最终消息也只需一次调用：" },
        { type: "code", code: `from anthropic import Anthropic\n\nclient = Anthropic(\n    base_url="${BASE}",\n    api_key="${KEY}",\n)\n\nwith client.messages.stream(\n    model="claude-sonnet-5",\n    max_tokens=1024,\n    messages=[{"role": "user", "content": "Explain SSE in one paragraph"}],\n) as stream:\n    for text in stream.text_stream:\n        print(text, end="", flush=True)\n    final = stream.get_final_message()\n    print(final.usage)  # input_tokens + final output_tokens` },
        { type: "code", code: `import Anthropic from "@anthropic-ai/sdk";\n\nconst client = new Anthropic({\n  baseURL: "${BASE}",\n  apiKey: "${KEY}",\n});\n\nconst stream = client.messages.stream({\n  model: "claude-sonnet-5",\n  max_tokens: 1024,\n  messages: [{ role: "user", content: "Explain SSE in one paragraph" }],\n});\nstream.on("text", (text) => process.stdout.write(text));\nconst final = await stream.finalMessage();\nconsole.log(final.usage);` },
        { type: "p", text: "不变的是钱：流式和非流式请求按同样方式计费——都按输入和输出 token——所以流式不会让你多花一分钱。500 个输出 token 的流式回答，和缓冲返回的同样 500 个 token 价格分毫不差；在控制台的用量明细里，两种方式显示的是同样的 token 行。改变的是感知延迟（首 token 只占总时长的一小部分）、长生成的韧性（空闲静默的非流式连接，正是代理和负载均衡器最喜欢掐断的那种），以及你的代码能多早做出反应——智能体可以在工具调用的右括号落地那一刻就派发调用，而不是等整个回复结束。" },
        { type: "list", items: [
          "聊天和编码界面：用户看着答案逐字出现——这就是“秒开”和“卡死”两种观感的差别。",
          "长生成：可以尽早渲染或处理部分输出，并让链路上每一跳都保持忙碌。",
          "智能体：一旦发出完整的工具调用就立即停手或分支。",
        ] },
        { type: "p", text: "对于短小的批处理任务——分类、抽取、任何几百 token 以内且没人盯着看的任务——非流式重试和记日志都更简单，成本也完全相同。无论选哪种模式，都要记住：流可能在 200 OK 之后才失败——在 message_stop 之前出现 event: error 或连接中断，都意味着生成没有完成。把累计的部分输出当作不可信数据——绝不要落库，也不要喂给智能体循环的下一步——直接重新发起请求。" },
        { type: "link", text: "用 Claude API 成本计算器估算流式工作负载的开销", href: "/tools/claude-api-cost-calculator" },
        { type: "link", text: "如果流在负载下因 429 失败，看限流指南", href: "/docs/learn/claude-api-rate-limits" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台奖励余额——适用于支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "apiToken.sale 支持 Claude API 流式输出吗？", a: `支持。在 POST ${BASE}/v1/messages 上设置 "stream": true，带上 x-api-key 和 anthropic-version 请求头，就会得到标准的 Anthropic SSE 事件流——message_start、content_block_delta 分块、带最终 usage 的 message_delta、message_stop。适用于编码智能体、IDE、官方 Anthropic Python 和 TypeScript SDK 的流式辅助方法，以及生产环境调用。` },
      { q: "流式获取 Claude 响应比非流式更贵吗？", a: "不会。流式和非流式请求按输入、输出 token 计费的方式完全相同，最终 usage 总数与缓冲响应一致——从终止的 message_delta 事件或控制台用量视图读取。流式只改变 token 到达你这里的时间，不改变它们的价格。" },
    ],
  };
