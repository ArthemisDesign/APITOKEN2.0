import type { ImageSeoSpec } from "./shared";
import { faq, list, note, paragraph, section, sharedCode, steps, table, tr, OPENAI } from "./shared";

export const spec: ImageSeoSpec = {
    slug: "gpt-image-2-api-cost",
    cluster: "explain",
    related: ["gpt-image-2-api-guide", "nano-banana-2-api-cost", "image-generation-api-pricing", "image-editing-api-guide"],
    title: tr(
      "GPT Image 2 API Cost: Rates and 50% Savings",
      "Стоимость GPT Image 2 API: ставки и скидка 50%",
      "GPT Image 2 API 成本：生成与编辑节省 50%",
      "GPT Image 2 API 비용: 생성과 편집 50% 절감",
    ),
    h1: tr(
      "GPT Image 2 API cost and 50% B2C savings",
      "Стоимость GPT Image 2 API и скидка 50% для B2C",
      "GPT Image 2 API 成本与 B2C 五折优惠",
      "GPT Image 2 API 비용과 B2C 50% 할인",
    ),
    description: tr(
      "GPT Image 2 API cost from real usage legs: official $5/$8/$30 per 1M tokens, a flat 50% B2C discount and worked per-request cost math.",
      "Стоимость GPT Image 2 API из реальных usage-составляющих: официальные $5/$8/$30 за 1M tokens, скидка 50% для B2C и готовый расчёт запроса.",
      "根据 text input、image input、cached input 与 image-output token 计算 GPT Image 2 生成和编辑成本，并应用 apiToken.sale B2C 五折。",
      "text input, image input, cached input, image-output token으로 GPT Image 2 생성·편집 비용과 apiToken.sale B2C 50% 할인을 계산하세요.",
    ),
    keywords: tr(
      ["gpt image 2 api cost", "gpt image 2 price", "cheap gpt image api", "gpt image 2 discount", "openai image generation cost", "gpt image edit price"],
      ["gpt image 2 цена api", "стоимость gpt image 2", "дешевый gpt image api", "скидка gpt image 2", "стоимость генерации openai", "цена gpt image edit"],
      ["gpt image 2 api 成本", "gpt image 2 价格", "便宜 gpt image api", "gpt image 2 折扣", "openai 图像生成成本", "gpt 图像编辑价格"],
      ["gpt image 2 api 비용", "gpt image 2 가격", "저렴한 gpt image api", "gpt image 2 할인", "openai 이미지 생성 비용", "gpt image edit 가격"],
    ),
    dek: tr(
      "GPT Image 2 has no honest fixed price per picture: every request is billed from terminal usage across text input, image input, cached input and image output — officially $5, $8 and $30 per million tokens on the fresh legs, with cached input at 25% of the fresh rate. A regular B2C account on apiToken.sale pays exactly half of that computed official total, so the same legs cost $2.50, $4 and $15 per million tokens here. Below: the full rate card, a worked cost example, and the measurement routine that keeps batch budgets honest.",
      "У GPT Image 2 нет честной фиксированной цены за картинку: каждый запрос оплачивается по terminal usage — text input, image input, cached input и image output, официально $5, $8 и $30 за миллион tokens на fresh-составляющих, а cached input равен 25% fresh-ставки. Обычный B2C-аккаунт на apiToken.sale платит ровно половину рассчитанной официальной суммы: те же составляющие стоят здесь $2.50, $4 и $15 за миллион tokens. Ниже — полная таблица ставок, просчитанный пример и процедура измерения, которая держит batch-бюджет под контролем.",
      "GPT Image 2 没有诚实的单张固定价格：每个请求都按终态 usage 结算，涵盖 text input、image input、cached input 与 image output——fresh 各项官方价分别为每百万 token $5、$8、$30，cached input 为 fresh 费率的 25%。apiToken.sale 普通 B2C 账户支付计算后官方总额的正好一半，即同样各项在这里为每百万 token $2.50、$4、$15。下文给出完整费率表、演算示例，以及让批量预算不失控的测量流程。",
      "GPT Image 2에는 정직한 이미지당 고정 가격이 없습니다. 모든 요청은 terminal usage로 정산되며 text input, image input, cached input, image output의 공식 fresh 요금은 백만 token당 각각 $5, $8, $30이고 cached input은 fresh 요금의 25%입니다. apiToken.sale 일반 B2C 계정은 계산된 공식 총액의 정확히 절반을 냅니다. 즉 같은 leg가 여기서는 백만 token당 $2.50, $4, $15입니다. 아래에서 전체 요금표, 실제 계산 예시, batch 예산을 지키는 측정 절차를 다룹니다.",
    ),
    sections: [
      section(
        tr("The five usage legs on every GPT Image 2 bill", "Пять составляющих каждого счёта GPT Image 2", "GPT Image 2 账单的五个计费项", "모든 GPT Image 2 청구서의 다섯 usage leg"),
        [
          table(
            { headers: ["Usage leg", "Official per 1M", "Regular B2C here"], rows: [["Fresh text input", "$5", "$2.50"], ["Fresh image input", "$8", "$4"], ["Cached text input", "$1.25", "$0.625"], ["Cached image input", "$2", "$1"], ["Image output", "$30", "$15"]] },
            { headers: ["Usage leg", "Официально за 1M", "Обычный B2C здесь"], rows: [["Fresh text input", "$5", "$2.50"], ["Fresh image input", "$8", "$4"], ["Cached text input", "$1.25", "$0.625"], ["Cached image input", "$2", "$1"], ["Image output", "$30", "$15"]] },
            { headers: ["Usage 项", "官方每 1M", "本站普通 B2C"], rows: [["Fresh text input", "$5", "$2.50"], ["Fresh image input", "$8", "$4"], ["Cached text input", "$1.25", "$0.625"], ["Cached image input", "$2", "$1"], ["Image output", "$30", "$15"]] },
            { headers: ["Usage leg", "공식 1M당", "일반 B2C 가격"], rows: [["Fresh text input", "$5", "$2.50"], ["Fresh image input", "$8", "$4"], ["Cached text input", "$1.25", "$0.625"], ["Cached image input", "$2", "$1"], ["Image output", "$30", "$15"]] },
          ),
          paragraph(
            "The bill for one request is sum(tokens in each leg × the leg's official rate) × 0.5 for a regular B2C account. Cached text and image input are billed at 25% of their fresh official rates before the discount — that is how $1.25 and $2 per million become $0.625 and $1 here. Nothing is rounded up per request: you pay for the exact tokens the terminal usage event reports, no per-call fee and no minimum image charge.",
            "Счёт за один запрос для обычного B2C: сумма(tokens каждого leg × official rate) × 0.5. Cached text и image input сначала считаются как 25% от fresh official rate — так $1.25 и $2 за миллион превращаются здесь в $0.625 и $1. Ничего не округляется вверх: вы платите за точные tokens из terminal usage event, без платы за вызов и минимальной цены картинки.",
            "普通 B2C 的单请求账单为：各项 token × 官方费率之和 × 0.5。cached text/image input 先按 fresh 官方费率的 25% 计算——这就是每百万 $1.25 与 $2 在这里变成 $0.625 与 $1 的原因。没有按次取整：你只为终态 usage event 报告的准确 token 付费，无按次费用，也无单张最低消费。",
            "일반 B2C의 요청 한 건 청구액은 각 leg token × 공식 요금의 합 × 0.5입니다. cached text/image input은 fresh 공식 요금의 25%로 먼저 계산되며, 그래서 백만당 $1.25와 $2가 여기서 $0.625와 $1이 됩니다. 건당 올림은 없습니다. terminal usage event가 보고한 정확한 token만 과금되고 호출당 수수료나 이미지 최소 요금도 없습니다.",
          ),
          paragraph(
            "A worked example. Suppose terminal usage for one edit request reports 400 fresh text-input tokens, 1,500 fresh image-input tokens from one PNG reference, 600 cached text-input tokens and 2,000 image-output tokens. Officially that is 400 × $5 + 1,500 × $8 + 600 × $1.25 + 2,000 × $30 per million = $0.0015 + $0.012 + $0.00075 + $0.06 = $0.07425, and the regular B2C charge is exactly half: $0.037125. The cached leg saved $0.00225 officially versus fresh input — real, but small next to the $0.06 output leg, which is why output discipline moves the budget more than cache hunting.",
            "Просчитанный пример. Допустим, terminal usage одного edit-запроса показывает 400 fresh text-input tokens, 1 500 fresh image-input tokens от одного PNG-reference, 600 cached text-input tokens и 2 000 image-output tokens. Официально это 400 × $5 + 1 500 × $8 + 600 × $1.25 + 2 000 × $30 на миллион = $0.0015 + $0.012 + $0.00075 + $0.06 = $0.07425, а обычный B2C платит ровно половину: $0.037125. Cached leg сэкономил $0.00225 официально по сравнению с fresh input — немного рядом с output-составляющей $0.06, поэтому дисциплина output влияет на бюджет сильнее, чем погоня за cache.",
            "演算示例。假设一次编辑请求的终态 usage 报告 400 个 fresh text-input token、一张 PNG 参考图带来的 1,500 个 fresh image-input token、600 个 cached text-input token 和 2,000 个 image-output token。官方价为 400 × $5 + 1,500 × $8 + 600 × $1.25 + 2,000 × $30（每百万）= $0.0015 + $0.012 + $0.00075 + $0.06 = $0.07425，普通 B2C 实付正好一半：$0.037125。cached 项相比 fresh input 官方节省 $0.00225——真实但远小于 $0.06 的输出项，所以输出纪律比追逐缓存更能控制预算。",
            "계산 예시. 한 edit 요청의 terminal usage가 fresh text-input token 400개, PNG reference 한 장의 fresh image-input token 1,500개, cached text-input token 600개, image-output token 2,000개를 보고한다고 가정합니다. 공식적으로는 백만당 400 × $5 + 1,500 × $8 + 600 × $1.25 + 2,000 × $30 = $0.0015 + $0.012 + $0.00075 + $0.06 = $0.07425이고 일반 B2C 청구액은 정확히 절반인 $0.037125입니다. cached leg가 fresh input 대비 공식 $0.00225를 절약했지만 $0.06 output leg에 비하면 작습니다. 그래서 cache 추적보다 output 관리가 예산을 더 크게 움직입니다.",
          ),
        ],
      ),
      section(
        tr("Generation vs. edit: the endpoint sets the input bill", "Generation или edit: endpoint определяет input-расход", "生成还是编辑：端点决定输入成本", "Generation과 edit: 엔드포인트가 input 비용을 결정"),
        [
          paragraph(
            "POST /v1/images/generations creates a new asset from a text prompt, so its bill is text input plus image output. POST /v1/images/edits accepts one to five strict PNG references, and every reference is metered as image input at $8/M officially — $4/M after the B2C discount. A single 2,000-token reference adds $0.016 officially ($0.008 here): cheap when it lifts acceptance, pure waste when the prompt alone would have passed review.",
            "POST /v1/images/generations создаёт новый ассет из text prompt, поэтому его счёт — это text input плюс image output. POST /v1/images/edits принимает 1–5 строгих PNG-references, и каждый reference тарифицируется как image input по $8/M официально — $4/M после B2C-скидки. Один reference на 2 000 tokens добавляет $0.016 официально ($0.008 здесь): дёшево, если повышает acceptance, и пустая трата, если prompt прошёл бы проверку сам.",
            "POST /v1/images/generations 根据文本提示词创建新资产，账单为 text input 加 image output。POST /v1/images/edits 接受 1–5 张严格 PNG 参考图，每张参考图按 image input 计费，官方 $8/M——B2C 折扣后 $4/M。一张 2,000 token 的参考图官方增加 $0.016（本站 $0.008）：能提升验收率时很便宜，提示词本身就能过审时则是纯浪费。",
            "POST /v1/images/generations는 text prompt로 새 asset을 만들므로 청구는 text input과 image output뿐입니다. POST /v1/images/edits는 1~5장의 strict PNG reference를 받으며 각 reference는 공식 $8/M(B2C 할인 후 $4/M)의 image input으로 계량됩니다. 2,000 token reference 한 장은 공식 $0.016(여기서 $0.008)을 더합니다. acceptance를 높일 때는 저렴하지만 prompt만으로 통과될 작업이면 낭비입니다.",
          ),
          list(
            ["Reach for generation when the asset is new and the prompt fully describes it; text input at $5/M officially is the cheapest leg on the card.", "Reach for edits when a reference anchors composition, brand color or product geometry — pay the image-input leg once instead of burning image-output tokens on retries.", "Keep references strict PNG and bounded in count: the endpoint accepts one to five, and each extra file is extra billed image input.", "Treat the shipped profile as fixed: one non-streaming PNG output with the documented opaque/low/auto controls; exact pixel dimensions are not promised on this subscription wire."],
            ["Используйте generation, когда ассет новый и prompt полностью его описывает: text input за $5/M официально — самая дешёвая составляющая таблицы.", "Используйте edits, когда reference фиксирует композицию, фирменный цвет или геометрию продукта: заплатите image-input один раз вместо сжигания image-output tokens на retries.", "References — строго PNG и в ограниченном количестве: endpoint принимает 1–5, и каждый лишний файл — это лишний оплаченный image input.", "Считайте подтверждённый профиль фиксированным: один non-streaming PNG с документированными controls opaque/low/auto; точные pixel dimensions этот subscription wire не обещает."],
            ["资产是新的且提示词能完整描述时用生成：官方 $5/M 的 text input 是费率表中最便宜的一项。", "当参考图能固定构图、品牌色或产品几何形状时用编辑：一次性支付 image-input，而不是在重试中烧掉 image-output token。", "参考图保持严格 PNG 且数量有界：端点接受 1–5 张，每多一个文件就多一份计费的 image input。", "把已发布配置视为固定：单张非流式 PNG，仅使用已记录的 opaque/low/auto 控制；该订阅传输不承诺准确像素尺寸。"],
            ["asset이 새 것이고 prompt가 완전히 설명하면 generation을 쓰세요. 공식 $5/M text input이 요금표에서 가장 저렴한 leg입니다.", "reference가 구도, 브랜드 컬러, 제품 형상을 고정할 때 edit을 쓰세요. retry로 image-output token을 태우는 대신 image-input을 한 번만 냅니다.", "reference는 strict PNG로, 개수는 제한해 두세요. 엔드포인트는 1~5장을 받고 파일 하나마다 과금되는 image input이 늘어납니다.", "배포된 profile은 고정으로 간주하세요. 문서화된 opaque/low/auto control의 non-streaming PNG 한 장이며 이 subscription wire는 정확한 pixel dimensions를 약속하지 않습니다."],
          ),
          tr(
            { type: "link", text: "Production image-editing workflow: references, validation, retries", href: "/docs/learn/image-editing-api-guide" },
            { type: "link", text: "Production-пайплайн редактирования: references, валидация, retries", href: "/docs/learn/image-editing-api-guide" },
            { type: "link", text: "生产级图像编辑工作流：参考图、验证与重试", href: "/docs/learn/image-editing-api-guide" },
            { type: "link", text: "프로덕션 이미지 편집 workflow: reference, 검증, retry", href: "/docs/learn/image-editing-api-guide" },
          ),
        ],
      ),
      section(
        tr("Measure one real request before budgeting a batch", "Измерьте один реальный запрос до бюджета на batch", "在为批量编制预算前，先测量一次真实请求", "batch 예산 전 실제 요청 한 건 측정"),
        [
          steps(
            ["Call gpt-image-2 on POST /v1/images/generations for a new asset, or /v1/images/edits when one to five strict PNG references genuinely improve the result.", "Keep the shipped profile bounded: a single non-streaming PNG output with only the documented controls — background opaque, quality low, size auto.", "Read terminal usage from the response instead of estimating output tokens from PNG bytes or dimensions; the usage event is the billing authority.", "Match the request against the dashboard charge and store its ID next to the generated asset, so per-image cost is a measured number, not folklore."],
            ["Вызовите gpt-image-2 через POST /v1/images/generations для нового ассета или /v1/images/edits, когда 1–5 строгих PNG-references действительно улучшают результат.", "Сохраняйте подтверждённый профиль ограниченным: один non-streaming PNG и только документированные controls — background opaque, quality low, size auto.", "Читайте terminal usage из ответа вместо оценки output tokens по размеру PNG или dimensions: billing authority — именно usage event.", "Сверьте запрос с charge в дашборде и храните его ID рядом с ассетом, чтобы цена картинки была измеренным числом, а не прикидкой."],
            ["新资产调用 gpt-image-2 的 POST /v1/images/generations；当 1–5 张严格 PNG 参考图确实改善结果时使用 /v1/images/edits。", "保持已发布配置有界：单张非流式 PNG，仅使用已记录的控制——background opaque、quality low、size auto。", "从响应中读取终态 usage，不要按 PNG 字节或尺寸估算输出 token；usage event 才是结算权威。", "将请求与仪表板扣费核对，并把 request ID 与生成资产一起保存，让单张成本成为实测数字而非估算。"],
            ["새 asset은 gpt-image-2의 POST /v1/images/generations를, 1~5장의 strict PNG reference가 실제로 결과를 개선할 때는 /v1/images/edits를 호출합니다.", "배포된 profile을 제한적으로 유지합니다. 문서화된 control(background opaque, quality low, size auto)만 쓰는 non-streaming PNG 한 장입니다.", "PNG bytes나 dimensions로 output token을 추정하지 말고 응답의 terminal usage를 읽으세요. billing authority는 usage event입니다.", "request를 dashboard charge와 대조하고 ID를 생성 asset 옆에 보관해 이미지당 비용을 추정이 아닌 측정값으로 만듭니다."],
          ),
          sharedCode(`curl ${OPENAI}/images/generations \\
  -H "Authorization: Bearer $APITOKEN_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{"model":"gpt-image-2","prompt":"A clean studio product photograph","background":"opaque","quality":"low","size":"auto"}'`),
          paragraph(
            "A trimmed response looks like this. Cost comes from the usage object, not from the payload size of b64_json:",
            "Сокращённый ответ выглядит так. Цена берётся из объекта usage, а не из размера b64_json:",
            "精简后的响应如下。成本来自 usage 对象，而不是 b64_json 的负载大小：",
            "다듬은 응답은 이렇습니다. 비용은 b64_json 페이로드 크기가 아니라 usage 객체에서 나옵니다.",
          ),
          sharedCode(`{
  "created": 1771000000,
  "data": [{ "b64_json": "…" }],
  "usage": {
    "total_tokens": 3900,
    "input_tokens": 1900,
    "input_tokens_details": { "text_tokens": 1000, "image_tokens": 900 },
    "output_tokens": 2000
  }
}`),
          note(
            "Do not publish a made-up price per image. Output usage varies between requests, this subscription wire does not promise exact dimensions, and the terminal usage event is the billing authority — quote measured numbers with their request IDs, or quote the rate card.",
            "Не публикуйте выдуманную цену картинки. Output usage меняется от запроса к запросу, этот subscription wire не обещает точные dimensions, а billing authority — terminal usage event: цитируйте измеренные числа вместе с request ID или таблицу ставок.",
            "不要发布虚构的单张价格。output usage 因请求而异，该订阅传输不承诺准确尺寸，结算权威是终态 usage event——引用实测数字时请带上 request ID，否则请引用费率表。",
            "가짜 이미지당 가격을 게시하지 마세요. output usage는 요청마다 다르고 이 subscription wire는 정확한 dimensions를 약속하지 않으며 billing authority는 terminal usage event입니다. 측정값은 request ID와 함께 인용하거나 요금표를 인용하세요.",
          ),
        ],
      ),
      section(
        tr("Cached input: a 75% discount you verify, not assume", "Cached input: скидка 75%, которую проверяют, а не предполагают", "Cached input：需要验证而非假设的 75% 折扣", "Cached input: 가정이 아니라 검증하는 75% 할인"),
        [
          paragraph(
            "Cached text input costs $1.25/M and cached image input $2/M officially — 25% of the fresh rates — before the same 50% B2C discount brings them to $0.625 and $1. On this route the provider decides what counts as cached; re-sending the same file does not guarantee the cached leg. The only proof is terminal usage reporting cached input for the request, so cache savings belong in the budget only after usage confirms them.",
            "Cached text input стоит $1.25/M, cached image input — $2/M официально, то есть 25% от fresh-ставок, а затем та же B2C-скидка 50% опускает их до $0.625 и $1. На этом маршруте провайдер сам решает, что считать cached: повторная отправка того же файла не гарантирует cached leg. Единственное доказательство — terminal usage, показывающий cached input для запроса, поэтому экономию на cache закладывайте в бюджет только после подтверждения usage.",
            "cached text input 官方 $1.25/M、cached image input 官方 $2/M——即 fresh 费率的 25%——再经同样 50% 的 B2C 折扣降至 $0.625 与 $1。在此路由上，是否计入缓存由提供方决定；重复发送同一文件并不保证 cached 项。唯一证据是终态 usage 报告该请求存在 cached input，因此只有 usage 确认后才能把缓存节省计入预算。",
            "cached text input은 공식 $1.25/M, cached image input은 공식 $2/M(fresh 요금의 25%)이며 같은 50% B2C 할인 후 $0.625와 $1이 됩니다. 이 route에서는 무엇이 cached인지 provider가 결정하므로 같은 파일을 다시 본낸다고 cached leg가 보장되지 않습니다. 유일한 증거는 해당 요청의 cached input을 보고하는 terminal usage이므로 cache 절감은 usage가 확인한 뒤에만 예산에 반영하세요.",
          ),
          list(
            ["Keep the stable prompt prefix byte-identical across batch items and push per-item variation to the end, so the shared prefix stays cache-eligible.", "Reuse the exact same reference bytes across a batch instead of re-encoding the PNG between calls.", "Score caching from the dashboard: compare cached legs in terminal usage across the batch and drop cache assumptions the usage does not confirm.", "Never budget at cached rates by default; budget at fresh rates and treat confirmed cache hits as upside."],
            ["Держите стабильный префикс prompt побайтово одинаковым во всех элементах batch, а вариативную часть переносите в конец — тогда общий префикс остаётся кандидатом на cache.", "Используйте в batch одни и те же байты reference, не перекодируя PNG между вызовами.", "Оценивайте cache по дашборду: сравнивайте cached legs в terminal usage по batch и отбрасывайте предположения, которые usage не подтверждает.", "Никогда не закладывайте cached-ставки в бюджет по умолчанию: считайте по fresh-ставкам, а подтверждённый cache считайте приятным бонусом."],
            ["让稳定提示词前缀在批量各项中逐字节一致，把单项差异放在末尾，使共享前缀保持可缓存。", "在批量中复用完全相同的参考图字节，不要在调用之间重新编码 PNG。", "用仪表板评估缓存：对比批量中各请求终态 usage 的 cached 项，放弃未被 usage 确认的缓存假设。", "默认不要按 cached 费率编制预算；按 fresh 费率预算，把已确认的缓存命中当作额外收益。"],
            ["안정적인 prompt prefix를 batch 항목 전체에서 byte 단위로 동일하게 유지하고 항목별 변동은 끝에 배치해 공유 prefix가 cache 후보로 남게 합니다.", "호출 사이에 PNG를 재인코딩하지 말고 batch 전체에서 정확히 같은 reference bytes를 재사용합니다.", "dashboard로 cache를 평가합니다. batch 전체 terminal usage의 cached leg를 비교하고 usage가 확인하지 못한 가정은 버립니다.", "기본 예산을 cached 요금으로 잡지 마세요. fresh 요금으로 예산을 잡고 확인된 cache 적중은 추가 이득으로 취급합니다."],
          ),
        ],
      ),
      section(
        tr("Cost controls that survive batch volume", "Контроль расходов при batch-объёмах", "经得起批量规模的成本控制", "batch 볼륨에서도 유지되는 비용 통제"),
        [
          paragraph(
            "The 50% figure is the regular B2C policy applied after official usage is calculated. B2B accounts follow their negotiated policy, and OpenKeys bill at official 1:1 prices — a discount headline never replaces reading your own account class.",
            "50% — это политика обычного B2C, применяемая после расчёта official usage. У B2B действует согласованная политика, а OpenKeys тарифицируются 1:1 по официальной цене: заголовок про скидку никогда не заменяет проверку класса собственного аккаунта.",
            "50% 是在官方 usage 计算完成后应用的普通 B2C 策略。B2B 账户遵循协商策略，OpenKeys 按官方价格 1:1 计费——折扣标题永远不能代替确认你自己的账户类型。",
            "50%는 official usage 계산 뒤 적용되는 일반 B2C 정책입니다. B2B 계정은 협상 정책을 따르고 OpenKeys는 공식 가격 1:1로 청구됩니다. 할인 문구가 내 계정 클래스 확인을 대신하지는 않습니다.",
          ),
          list(
            ["Use generation for new assets and edits only where references measurably raise acceptance; each reference adds image-input cost.", "Reject unusable outputs against a fixed visual checklist and cap retries per asset — endless prompt retries erase a nominal token discount.", "Give the image worker its own API key with a lifetime spending limit so a runaway batch cannot drain the whole account balance.", "Reconcile terminal usage against dashboard charges after the first calls and on a schedule; per-image cost should come from measured usage, not from a blog average."],
            ["Для новых ассетов используйте generation, а edits — только там, где references измеримо повышают acceptance: каждый reference добавляет image-input cost.", "Отклоняйте непригодные результаты фиксированным visual checklist и ограничивайте retries на ассет: бесконечные prompt retries съедают номинальную скидку.", "Выдайте image-worker отдельный API-ключ с общим lifetime spending limit, чтобы сбежавший batch не обнулил баланс аккаунта.", "Сверяйте terminal usage с charges в дашборде после первых вызовов и по расписанию: цена картинки должна браться из измеренного usage, а не из средних по блогам."],
            ["新资产使用生成；仅当参考图能可测量地提升验收率时使用编辑——每张参考图都会增加 image-input 成本。", "用固定视觉清单拒绝不可用输出，并限制每个资产的重试次数——无尽提示词重试会抹掉名义折扣。", "为图像 worker 配置独立 API 密钥与终身消费上限，避免失控的批量耗尽整个账户余额。", "在首批调用后并定期将终态 usage 与仪表板扣费核对；单张成本应来自实测 usage，而不是博客平均值。"],
            ["새 asset에는 generation을 쓰고 reference가 acceptance를 측정 가능하게 높일 때만 edit을 사용합니다. reference마다 image-input 비용이 붙습니다.", "고정 visual checklist로 실패 output을 거부하고 asset당 retry 횟수를 제한하세요. 무한 prompt retry는 명목 할인을 지웁니다.", "image worker 전용 API key에 평생 누적 지출 한도를 둬서 폭주한 batch가 계정 잔액 전체를 소진하지 못하게 합니다.", "첫 호출 이후와 정기적으로 terminal usage를 dashboard charge와 대조하세요. 이미지당 비용은 블로그 평균이 아니라 측정된 usage에서 나와야 합니다."],
          ),
          note(
            "New accounts created with Google or GitHub start with $5 of platform bonus credit — enough to measure a real GPT Image 2 workload before paying anything. Top-ups accept bank cards and cryptocurrency in whole-dollar amounts, so you can fund exactly the budget your measurement produced.",
            "Новые аккаунты, созданные через Google или GitHub, получают $5 бонусного баланса платформы — этого хватит, чтобы измерить реальную нагрузку GPT Image 2 до оплаты. Пополнение принимает банковские карты и криптовалюту целыми долларовыми суммами, так что можно внести ровно тот бюджет, который показало измерение.",
            "通过 Google 或 GitHub 创建的新账户可获 $5 平台奖励余额——足以在付费前测量真实的 GPT Image 2 工作负载。充值支持银行卡与加密货币，金额为整数美元，可以正好充入测量得出的预算。",
            "Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 보너스 크레딧으로 시작해 결제 전 실제 GPT Image 2 워크로드를 측정하기에 충분합니다. 충전은 은행 카드와 암호화폐를 정수 달러 금액으로 받으므로 측정된 예산만큼 정확히 충전할 수 있습니다.",
          ),
          tr(
            { type: "link", text: "GPT Image 2 model page: rates, snapshot alias and limits", href: "/models/gpt-image-2" },
            { type: "link", text: "Страница модели GPT Image 2: ставки, alias снапшота и лимиты", href: "/models/gpt-image-2" },
            { type: "link", text: "GPT Image 2 模型页：费率、快照别名与限额", href: "/models/gpt-image-2" },
            { type: "link", text: "GPT Image 2 모델 페이지: 요금, 스냅샷 alias, 제한", href: "/models/gpt-image-2" },
          ),
        ],
      ),
    ],
    faq: [
      faq(
        tr("How much does GPT Image 2 cost on apiToken.sale?", "Сколько стоит GPT Image 2 на apiToken.sale?", "apiToken.sale 上 GPT Image 2 多少钱？", "apiToken.sale에서 GPT Image 2 비용은 얼마인가요?"),
        tr("A regular B2C account pays $2.50/M fresh text input, $4/M fresh image input and $15/M image output — exactly half of the official $5, $8 and $30 per million. Cached input is a quarter of the fresh official rate before the same 50% discount, landing at $0.625/M for text and $1/M for images. The settled total for any request follows its terminal usage.", "Обычный B2C-аккаунт платит $2.50/M за fresh text input, $4/M за fresh image input и $15/M за image output — ровно половину официальных $5, $8 и $30 за миллион. Cached input сначала равен четверти fresh official rate, затем получает ту же скидку 50%: $0.625/M за text и $1/M за images. Итог любого запроса определяется его terminal usage.", "普通 B2C 账户 fresh text input 为 $2.50/M、fresh image input 为 $4/M、image output 为 $15/M——正好是官方每百万 $5、$8、$30 的一半。cached input 先按 fresh 官方费率的四分之一计算，再享同样五折：文本 $0.625/M，图像 $1/M。任何请求的结算总额以其终态 usage 为准。", "일반 B2C 계정은 fresh text input $2.50/M, fresh image input $4/M, image output $15/M을 냅니다. 공식 백만당 $5, $8, $30의 정확히 절반입니다. cached input은 fresh 공식 요금의 1/4로 계산한 뒤 같은 50% 할인을 받아 text $0.625/M, image $1/M이 됩니다. 모든 요청의 정산액은 terminal usage를 따릅니다."),
      ),
      faq(
        tr("Why is there no fixed price for one GPT Image 2 picture?", "Почему нет фиксированной цены одной картинки GPT Image 2?", "为什么没有 GPT Image 2 单张固定价格？", "왜 GPT Image 2 이미지 한 장의 고정 가격이 없나요?"),
        tr("Because billing combines the actual text input, optional PNG references, any cached input and the image-output tokens reported for that specific request, and output usage varies between prompts. PNG byte size and pixel dimensions are not the billing formula — the terminal usage event is.", "Потому что счёт складывается из фактического text input, необязательных PNG-references, cached input и image-output tokens конкретного запроса, а output usage меняется от prompt к prompt. Размер PNG в байтах и pixel dimensions — не формула цены: ею является terminal usage event.", "因为账单由实际 text input、可选 PNG 参考图、cached input 以及该请求报告的 image-output token 组合而成，且 output usage 随提示词变化。PNG 字节大小和像素尺寸都不是计费公式——终态 usage event 才是。", "실제 text input, 선택적 PNG reference, cached input, 해당 요청에 보고된 image-output token을 합산해 청구하고 output usage는 prompt마다 달라지기 때문입니다. PNG byte 크기나 pixel dimensions는 과금 공식이 아니며 terminal usage event가 공식입니다."),
      ),
      faq(
        tr("Are edits included in the 50% discount?", "На edits тоже действует скидка 50%?", "编辑请求也享受五折吗？", "edit에도 50% 할인이 적용되나요?"),
        tr("Yes for regular B2C: the policy applies to the complete official cost of an edit, including image-input references and image output. B2B accounts follow their negotiated policy, and OpenKeys bill at official 1:1 prices.", "Да, для обычного B2C: политика применяется ко всей официальной стоимости edit, включая image-input references и image output. У B2B действует согласованная политика, а OpenKeys тарифицируются 1:1 по официальной цене.", "普通 B2C 是：策略应用于 edit 的完整官方成本，包括参考图 image input 与 image output。B2B 账户遵循协商策略，OpenKeys 按官方价格 1:1 计费。", "일반 B2C에는 적용됩니다. reference image input과 image output을 포함한 edit 전체 공식 비용에 정책이 적용됩니다. B2B 계정은 협상 정책을 따르고 OpenKeys는 공식 가격 1:1로 청구됩니다."),
      ),
      faq(
        tr("Does GPT Image 2 support transparent backgrounds here?", "Поддерживает ли GPT Image 2 прозрачный фон здесь?", "本站 GPT Image 2 支持透明背景吗？", "여기 GPT Image 2는 투명 배경을 지원하나요?"),
        tr("No. The proved public profile supports the opaque/low/auto controls and one PNG output per request; a transparent background is rejected rather than silently approximated, so design and budget around opaque output.", "Нет. Подтверждённый public profile поддерживает controls opaque/low/auto и один PNG на запрос; transparent background отклоняется, а не молча аппроксимируется, поэтому закладывайте в дизайн и бюджет непрозрачный результат.", "不支持。已验证的公开配置支持 opaque/low/auto 控制与单请求单张 PNG；透明背景会被拒绝，而不是被静默近似，因此请按不透明输出做设计和预算。", "아닙니다. 검증된 public profile은 opaque/low/auto control과 요청당 PNG 한 장을 지원합니다. 투명 배경은 조용히 근사되지 않고 거부되므로 불투명 출력을 기준으로 디자인과 예산을 잡으세요."),
      ),
      faq(
        tr("Can I measure GPT Image 2 cost before paying?", "Можно ли измерить стоимость GPT Image 2 до оплаты?", "能否在付费前测量 GPT Image 2 成本？", "결제 전에 GPT Image 2 비용을 측정할 수 있나요?"),
        tr("Yes. New accounts created with Google or GitHub start with $5 of platform bonus credit — real metered balance you can spend on GPT Image 2 requests while reading terminal usage. Email and password accounts do not receive the bonus.", "Да. Новые аккаунты, созданные через Google или GitHub, получают $5 бонусного баланса платформы — реальные тарифицируемые средства, которые можно тратить на запросы GPT Image 2, читая terminal usage. Аккаунты по email и паролю бонус не получают.", "可以。通过 Google 或 GitHub 创建的新账户可获 $5 平台奖励余额——这是真实的计费余额，可用于 GPT Image 2 请求并读取终态 usage。邮箱密码账户不享受该奖励。", "네. Google 또는 GitHub로 만든 신규 계정은 실제 과금 잔액인 $5 플랫폼 보너스 크레딧으로 시작해 GPT Image 2 요청에 쓰며 terminal usage를 읽을 수 있습니다. 이메일/비밀번호 계정은 보너스를 받지 않습니다."),
      ),
    ],
  };
