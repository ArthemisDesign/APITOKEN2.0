import type { ImageSeoSpec } from "./shared";
import { faq, list, note, paragraph, section, sharedCode, steps, table, tr, OPENAI, ROUTER } from "./shared";

export const spec: ImageSeoSpec = {
    slug: "image-generation-api-pricing",
    cluster: "explain",
    related: ["nano-banana-2-api-cost", "gpt-image-2-api-cost", "cheapest-image-generation-api", "how-billing-works"],
    title: tr(
      "Image Generation API Pricing: Rates and Discounts",
      "Цены API генерации изображений: ставки и скидки",
      "图像生成 API 定价：Token 费率、尺寸与折扣",
      "이미지 생성 API 가격: token 요금, 크기, 할인",
    ),
    h1: tr(
      "How image generation API pricing actually works",
      "Как на самом деле устроена цена API генерации изображений",
      "图像生成 API 的真实计价方式",
      "이미지 생성 API 가격의 실제 계산 방식",
    ),
    description: tr(
      "Image generation API pricing explained: usage legs, official token rates, fixed 1K/2K/4K image costs, cached input and the 50% B2C discount.",
      "Цены API генерации изображений: составляющие usage, официальные ставки за tokens, фиксированная цена 1K/2K/4K, cached input и скидка 50%.",
      "了解 Nano Banana 2 与 GPT Image 2 的 AI 图像 API 定价：usage 项、官方 token 费率、1K/2K/4K 固定图像成本、cached input、B2C 五折及每个验收资产成本。",
      "Nano Banana 2와 GPT Image 2의 AI image API 가격: usage leg, 공식 token 요금, 고정 1K/2K/4K 이미지 비용, cached input, B2C 50% 할인과 accepted asset당 비용을 이해하세요.",
    ),
    keywords: tr(
      ["image generation api pricing", "ai image api cost", "image generator api price", "image generation token pricing", "cheap image api", "ai image generation discount"],
      ["цена api генерации изображений", "стоимость ai image api", "image generator api цена", "image generation token pricing", "дешевый image api", "скидка ai генерация"],
      ["图像生成 api 定价", "ai 图像 api 成本", "图像生成器 api 价格", "图像生成 token 定价", "便宜图像 api", "ai 图像生成折扣"],
      ["이미지 생성 api 가격", "ai image api 비용", "image generator api 가격", "image generation token 가격", "저렴한 image api", "ai 이미지 생성 할인"],
    ),
    dek: tr(
      "No image API sells an honest per-picture price: every request settles as metered usage legs, and the account's pricing policy applies to that exact official total. The unit worth budgeting is cost per accepted asset, including retries and rejected outputs.",
      "Ни один image API не продаёт честную цену «за картинку»: каждый запрос рассчитывается как metered usage legs, а pricing policy аккаунта применяется к этому exact official total. Единица для бюджета — cost per accepted asset, включая retries и отклонённые outputs.",
      "没有任何图像 API 提供诚实的按张定价：每个请求都按计量 usage 项结算，账户定价策略再应用于准确的官方总额。值得做预算的单位是每个验收资产成本，包括重试与被拒输出。",
      "어떤 image API도 정직한 이미지당 가격을 팔지 않습니다. 모든 요청은 metered usage leg로 정산되고 계정 pricing policy가 exact official total에 적용됩니다. 예산 단위는 retry와 rejected output을 포함한 accepted asset당 비용입니다.",
    ),
    sections: [
      section(
        tr("Image APIs bill usage legs, not pictures", "Image API тарифицирует usage legs, а не картинки", "图像 API 按 usage 项计费，而非按张", "이미지 API는 그림이 아닌 usage leg로 과금"),
        [
          paragraph(
            "Both published image models on apiToken.sale — Nano Banana 2 (gemini-3.1-flash-image) and GPT Image 2 (gpt-image-2) — bill the same way: text input, optional reference-image input, cached input where the provider recognizes it, and image-output tokens are metered at official rates, and then the account's policy is applied to that exact official total. For a regular B2C account the policy is a flat 50% off, so the legs below are the entire pricing story.",
            "Обе опубликованные image-модели на apiToken.sale — Nano Banana 2 (gemini-3.1-flash-image) и GPT Image 2 (gpt-image-2) — тарифицируются одинаково: text input, необязательный reference-image input, cached input, если provider его распознал, и image-output tokens считаются по официальным ставкам, а затем к этому exact official total применяется политика аккаунта. Для обычного B2C это фиксированные 50% скидки, поэтому legs ниже — вся история цены.",
            "apiToken.sale 上已发布的两款图像模型——Nano Banana 2（gemini-3.1-flash-image）与 GPT Image 2（gpt-image-2）——计费方式相同：text input、可选的 reference-image input、提供商认可的 cached input 以及 image-output token 按官方费率计量，然后账户策略应用于该准确官方总额。普通 B2C 账户享受固定五折，因此下表的各项就是完整的定价逻辑。",
            "apiToken.sale의 두 공개 이미지 모델 Nano Banana 2(gemini-3.1-flash-image)와 GPT Image 2(gpt-image-2)는 같은 방식으로 과금됩니다. text input, 선택적 reference-image input, provider가 인정한 cached input, image-output token이 공식 요금으로 계량된 뒤 계정 정책이 exact official total에 적용됩니다. 일반 B2C는 고정 50% 할인이므로 아래 leg가 가격의 전부입니다.",
          ),
          table(
            { headers: ["Component", "Nano Banana 2", "GPT Image 2"], rows: [["Model ID", "gemini-3.1-flash-image", "gpt-image-2"], ["Prompt/text input", "$0.50/M official", "$5/M official"], ["Reference image input", "input tokens at model rate", "$8/M official"], ["Rendered image", "$60/M image tokens; fixed counts by size: 1,120 (1K), 1,680 (2K), 2,520 (4K)", "$30/M actual image-output tokens from terminal usage"], ["Cached input", "no image-model input discount", "25% of fresh: $1.25/M text, $2/M image"], ["Regular B2C here", "50% off exact official total", "50% off exact official total"]] },
            { headers: ["Компонент", "Nano Banana 2", "GPT Image 2"], rows: [["Model ID", "gemini-3.1-flash-image", "gpt-image-2"], ["Prompt/text input", "$0.50/M official", "$5/M official"], ["Reference image input", "input tokens по ставке модели", "$8/M official"], ["Rendered image", "$60/M image tokens; фиксированные значения по size: 1 120 (1K), 1 680 (2K), 2 520 (4K)", "$30/M actual image-output tokens из terminal usage"], ["Cached input", "нет input-скидки image-модели", "25% от fresh: $1.25/M text, $2/M image"], ["Обычный B2C здесь", "50% от exact official total", "50% от exact official total"]] },
            { headers: ["组成", "Nano Banana 2", "GPT Image 2"], rows: [["Model ID", "gemini-3.1-flash-image", "gpt-image-2"], ["Prompt/text input", "$0.50/M 官方", "$5/M 官方"], ["参考图输入", "按模型费率计算 input token", "$8/M 官方"], ["渲染图像", "$60/M image token；按尺寸固定数量：1,120（1K）、1,680（2K）、2,520（4K）", "$30/M 终态 usage 中的实际 image-output token"], ["Cached input", "该图像模型无 input 折扣", "fresh 的 25%：text $1.25/M、image $2/M"], ["本站普通 B2C", "准确官方总额五折", "准确官方总额五折"]] },
            { headers: ["구성", "Nano Banana 2", "GPT Image 2"], rows: [["Model ID", "gemini-3.1-flash-image", "gpt-image-2"], ["Prompt/text input", "$0.50/M 공식", "$5/M 공식"], ["Reference image input", "모델 요금의 input token", "$8/M 공식"], ["Rendered image", "$60/M image token; 크기별 고정 수량: 1,120(1K), 1,680(2K), 2,520(4K)", "$30/M terminal usage의 실제 image-output token"], ["Cached input", "image-model input 할인 없음", "fresh의 25%: text $1.25/M, image $2/M"], ["일반 B2C 가격", "exact official total의 50%", "exact official total의 50%"]] },
          ),
          tr(
            { type: "link", text: "Live model catalog with per-model token rates", href: "/models" },
            { type: "link", text: "Живой каталог моделей со ставками за tokens", href: "/models" },
            { type: "link", text: "实时模型目录，含各模型 token 费率", href: "/models" },
            { type: "link", text: "모델별 token 요금이 있는 라이브 모델 카탈로그", href: "/models" },
          ),
        ],
      ),
      section(
        tr("Price the exact request and response shape", "Считайте цену по точной форме request и response", "按准确的请求与响应形态计价", "정확한 request/response 형태로 가격 계산"),
        [
          paragraph(
            "The two models follow the same pricing rule but are reached through different protocols. Nano Banana 2 uses the native Gemini generateContent shape with the x-goog-api-key header and takes output size as an explicit imageSize control (1K, 2K or 4K) plus an aspect ratio. GPT Image 2 uses the OpenAI Images routes with Authorization: Bearer and the published controls background opaque, quality low and size auto; edits go to /v1/images/edits with one to five strict PNG references.",
            "Обе модели подчиняются одному правилу цены, но доступны по разным protocols. Nano Banana 2 использует native Gemini generateContent с заголовком x-goog-api-key и принимает размер output как явный imageSize control (1K, 2K или 4K) плюс aspect ratio. GPT Image 2 использует OpenAI Images routes с Authorization: Bearer и опубликованными controls background opaque, quality low и size auto; edits идут на /v1/images/edits с 1–5 строгими PNG references.",
            "两款模型遵循同一定价规则，但协议不同。Nano Banana 2 使用原生 Gemini generateContent，带 x-goog-api-key 头，输出尺寸由显式 imageSize 控制（1K、2K 或 4K）并指定宽高比。GPT Image 2 使用 OpenAI Images 路由，带 Authorization: Bearer，已发布控制为 background opaque、quality low、size auto；编辑走 /v1/images/edits，参考图为 1–5 张严格 PNG。",
            "두 모델은 같은 가격 규칙을 따르지만 protocol이 다릅니다. Nano Banana 2는 x-goog-api-key 헤더의 native Gemini generateContent를 쓰고 output 크기를 명시적 imageSize control(1K, 2K, 4K)과 aspect ratio로 받습니다. GPT Image 2는 Authorization: Bearer의 OpenAI Images route를 쓰며 공개 control은 background opaque, quality low, size auto이고 edit은 1~5 strict PNG reference와 함께 /v1/images/edits로 갑니다.",
          ),
          sharedCode(`curl ${ROUTER}/v1beta/models/gemini-3.1-flash-image:generateContent \\
  -H "x-goog-api-key: $APITOKEN_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{"contents":[{"parts":[{"text":"A studio photo of a ceramic mug"}]}],"generationConfig":{"responseModalities":["TEXT","IMAGE"],"imageConfig":{"imageSize":"1K","aspectRatio":"1:1"}}}'

curl ${OPENAI}/images/generations \\
  -H "Authorization: Bearer $APITOKEN_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{"model":"gpt-image-2","prompt":"A studio photo of a ceramic mug","background":"opaque","quality":"low","size":"auto"}'`),
          note(
            "The response shapes differ too: Nano Banana 2 returns the picture as an inlineData image part, GPT Image 2 as one non-streaming base64 PNG. In both cases a delivered response is settled usage — there is no free preview call that returns pixels.",
            "Формы response тоже различаются: Nano Banana 2 возвращает картинку как inlineData image part, GPT Image 2 — как один non-streaming base64 PNG. В обоих случаях доставленный response — это settled usage: бесплатного preview-вызова, возвращающего пиксели, не существует.",
            "响应形态也不同：Nano Banana 2 以 inlineData 图像 part 返回图片，GPT Image 2 返回单张非流式 base64 PNG。两种情况下已交付响应都属于已结算 usage——不存在返回图像的免费预览调用。",
            "response 형태도 다릅니다. Nano Banana 2는 그림을 inlineData image part로, GPT Image 2는 non-streaming base64 PNG 한 장으로 반환합니다. 두 경우 모두 delivery된 response는 settled usage이며 픽셀을 반환하는 묵료 preview call은 없습니다.",
          ),
        ],
      ),
      section(
        tr("Worked cost math on the published rates", "Расчёт цены по опубликованным ставкам", "按已发布费率演算成本", "공개 요금으로 비용 계산"),
        [
          paragraph(
            "Nano Banana 2 makes the arithmetic fully predictable. A 1K render is fixed at 1,120 image tokens: 1,120 × $60/M = $0.0672 official, or $0.0336 after the B2C half. A 400-token prompt adds 400 × $0.50/M = $0.0002, so the whole request settles at about $0.0674 official and $0.0337 payable. The same fixed legs price 2K output at $0.0504 and 4K at $0.0756 after the discount. Text or thinking output and grounding, when used, remain separate legs in terminal usage.",
            "У Nano Banana 2 арифметика полностью предсказуема. Рендер 1K зафиксирован на 1 120 image tokens: 1 120 × $60/M = $0.0672 official, или $0.0336 после B2C-половины. Prompt на 400 tokens добавляет 400 × $0.50/M = $0.0002, поэтому весь запрос рассчитывается примерно как $0.0674 official и $0.0337 payable. Те же фиксированные legs дают после скидки $0.0504 за 2K и $0.0756 за 4K. Text/thinking output и grounding, если используются, остаются отдельными legs в terminal usage.",
            "Nano Banana 2 的算术完全可预测。1K 渲染固定为 1,120 image token：1,120 × $60/M = $0.0672 官方，五折后 $0.0336。400 token 的提示词增加 400 × $0.50/M = $0.0002，整个请求约结算为 $0.0674 官方、应付约 $0.0337。同样的固定项在折后给出 2K $0.0504、4K $0.0756。文本/思考输出与 grounding（如使用）在终态 usage 中仍是独立计费项。",
            "Nano Banana 2는 계산이 완전히 예측 가능합니다. 1K 렌더는 1,120 image token으로 고정되어 1,120 × $60/M = $0.0672 공식, B2C 절반 적용 후 $0.0336입니다. 400 token prompt는 400 × $0.50/M = $0.0002를 더하므로 전체 요청은 약 $0.0674 공식, $0.0337 payable로 정산됩니다. 같은 고정 leg로 할인 후 2K는 $0.0504, 4K는 $0.0756입니다. 사용 시 text/thinking output과 grounding은 terminal usage에서 별도 leg로 남습니다.",
          ),
          paragraph(
            "GPT Image 2 keeps the legs variable, so work through an example and then measure. Suppose a product edit sends 900 fresh text input tokens ($0.0045 official), one reference that tokenizes to 1,200 fresh image input tokens ($0.0096), and returns 4,000 image-output tokens ($0.12). The official total is $0.1341 and regular B2C pays about $0.0671. Your real request will differ — terminal usage, not this arithmetic, is the billing authority.",
            "У GPT Image 2 legs переменные, поэтому разберите пример, а затем измерьте. Допустим, product edit отправляет 900 fresh text input tokens ($0.0045 official), одну reference, которая превращается в 1 200 fresh image input tokens ($0.0096), и возвращает 4 000 image-output tokens ($0.12). Official total — $0.1341, обычный B2C платит около $0.0671. Ваш реальный запрос будет другим: billing authority — terminal usage, а не эта арифметика.",
            "GPT Image 2 的计费项是可变的，因此先演算示例再实测。假设一次产品编辑发送 900 fresh text input token（官方 $0.0045）、一张折算为 1,200 fresh image input token 的参考图（$0.0096），并返回 4,000 image-output token（$0.12）。官方总额为 $0.1341，普通 B2C 约付 $0.0671。真实请求会不同——结算权威是终态 usage，而不是这里的演算。",
            "GPT Image 2는 leg가 가변적이므로 예시를 계산한 뒤 측정하세요. product edit이 fresh text input 900 token(공식 $0.0045), fresh image input 1,200 token으로 환산되는 reference 한 장($0.0096)을 보내고 image-output 4,000 token($0.12)을 반환한다고 가정하면 official total은 $0.1341이고 일반 B2C는 약 $0.0671을 냅니다. 실제 요청은 다를 수 있으며 billing authority는 이 계산이 아닌 terminal usage입니다.",
          ),
          paragraph(
            "The unit the business should budget is cost per accepted asset: all settled request charges, including rejected outputs, divided by accepted assets. If 60% of the 1K Nano Banana 2 candidates above pass review, each accepted asset really costs about $0.0337 ÷ 0.6 ≈ $0.056. A workflow that looks cheap per token but needs three or four attempts per keeper loses to a nominally pricier model with a higher acceptance rate.",
            "Единица, которую бизнес должен закладывать в бюджет, — cost per accepted asset: все settled request charges, включая rejected outputs, делённые на принятые ассеты. Если 60% кандидатов 1K Nano Banana 2 из примера проходят review, каждый принятый ассет на самом деле стоит около $0.0337 ÷ 0.6 ≈ $0.056. Workflow, который выглядит дешёвым по token rate, но требует три-четыре attempts на keeper, проигрывает номинально более дорогой модели с более высоким acceptance rate.",
            "业务应做预算的单位是每个验收资产成本：所有已结算请求费用（含被拒输出）除以验收资产数。如果上例中 60% 的 1K Nano Banana 2 候选通过审核，每个验收资产实际约为 $0.0337 ÷ 0.6 ≈ $0.056。单 token 看似便宜、但每个可用结果需要三四次尝试的工作流，会输给名义更贵但验收率更高的模型。",
            "비즈니스가 예산으로 잡아야 할 단위는 accepted asset당 비용입니다. rejected output을 포함한 모든 settled request charge를 accepted asset 수로 나눕니다. 위 1K Nano Banana 2 candidate의 60%가 review를 통과하면 accepted asset당 실제 비용은 약 $0.0337 ÷ 0.6 ≈ $0.056입니다. token rate는 싸 보여도 keeper당 3~4회 attempt가 필요한 workflow는 명목상 더 비싸지만 acceptance rate가 높은 모델에 집니다.",
          ),
        ],
      ),
      section(
        tr("Account class decides what the same usage costs", "Класс аккаунта определяет цену того же usage", "账户类型决定相同 usage 的价格", "계정 등급이 같은 usage의 가격을 결정"),
        [
          table(
            { headers: ["Account class", "Pricing rule"], rows: [["Regular B2C", "Global 50% discount on the exact official total, then any more-specific valid rule"], ["B2B", "Only its negotiated provider/model policy"], ["OpenKeys", "Official 1:1 pricing; no B2C discount"], ["Service", "Meter-only; no customer charge"]] },
            { headers: ["Класс аккаунта", "Pricing rule"], rows: [["Обычный B2C", "Глобальная скидка 50% на exact official total, затем более specific valid rule"], ["B2B", "Только согласованная provider/model policy"], ["OpenKeys", "Official 1:1; без B2C-скидки"], ["Service", "Meter-only; customer charge не вычисляется"]] },
            { headers: ["账户类型", "定价规则"], rows: [["普通 B2C", "准确官方总额全局五折，再应用更具体的有效规则"], ["B2B", "仅使用协商后的 provider/model 策略"], ["OpenKeys", "官方 1:1；无 B2C 折扣"], ["Service", "仅计量；不计算客户扣费"]] },
            { headers: ["계정 등급", "Pricing rule"], rows: [["일반 B2C", "exact official total에 글로벌 50% 할인 후 더 구체적인 valid rule"], ["B2B", "협상된 provider/model policy만"], ["OpenKeys", "공식 1:1; B2C 할인 없음"], ["Service", "Meter-only; customer charge 없음"]] },
          ),
          paragraph(
            "Charges draw down a prepaid balance rather than an invoice. Accounts created with Google or GitHub start with a $5 platform bonus credit that is spent before any paid balance — enough for countTokens estimates and several bounded image generations. Top-ups accept any whole-dollar amount by bank card or cryptocurrency, and the balance never expires.",
            "Списания идут с prepaid balance, а не по invoice. Аккаунты, созданные через Google или GitHub, стартуют с platform bonus credit $5, который расходуется раньше paid balance, — этого хватит на countTokens-оценки и несколько bounded image generations. Пополнение принимает любую whole-dollar сумму банковской картой или криптовалютой, а баланс не сгорает.",
            "扣费从预付余额中划扣，而非账单纯结算。通过 Google 或 GitHub 创建的账户自带 $5 平台赠金，先于任何付费余额消耗——足以完成 countTokens 估算和数次有界图像生成。充值支持银行卡或加密货币的任意整数美元金额，余额永不过期。",
            "charge는 invoice가 아니라 prepaid balance에서 차감됩니다. Google이나 GitHub로 만든 계정은 paid balance보다 먼저 쓰이는 $5 platform bonus credit으로 시작하며, countTokens 추정과 여러 bounded image generation에 충분합니다. 충전은 은행 카드나 암호화폐로 임의의 whole-dollar 금액을 받고 잔액은 만료되지 않습니다.",
          ),
          note(
            "A discount changes the payable amount, not model availability. Always discover the model with the same key before building a budget or publishing an availability claim.",
            "Скидка меняет payable amount, а не model availability. Всегда проверяйте модель тем же ключом до бюджета или публичного availability claim.",
            "折扣改变应付金额，不改变模型可用性。制定预算或发布可用性声明前，必须使用同一密钥发现模型。",
            "할인은 payable amount를 바꾸지만 model availability는 바꾸지 않습니다. 예산이나 availability claim 전에 같은 key로 모델을 discovery하세요.",
          ),
          tr(
            { type: "link", text: "How prepaid billing, the $5 bonus and key limits work", href: "/docs/learn/how-billing-works" },
            { type: "link", text: "Как устроены prepaid-биллинг, бонус $5 и лимиты ключа", href: "/docs/learn/how-billing-works" },
            { type: "link", text: "预付计费、$5 赠金与密钥限额的工作方式", href: "/docs/learn/how-billing-works" },
            { type: "link", text: "prepaid 결제, $5 별도 크레딧, key 한도 작동 방식", href: "/docs/learn/how-billing-works" },
          ),
        ],
      ),
      section(
        tr("Budget a campaign you can defend", "Бюджет кампании, который можно защитить", "建立可辩护的活动预算", "방어 가능한 캠페인 예산"),
        [
          steps(
            ["Discover the exact image model and its protocol with the production key.", "Estimate input for free: countTokens on gemini-3.1-flash-image for Nano Banana 2, or one bounded real request with terminal usage for GPT Image 2.", "Cap the request shape: output size (1K/2K/4K or auto), reference count and maximum attempts per asset.", "Record terminal usage, request ID and the discounted charge for every settled attempt, and reconcile them against the dashboard ledger entry.", "Measure acceptance rate on a representative set, multiply cost per accepted asset by forecast volume, and set the key's lifetime spending limit below that budget with an alert before exhaustion."],
            ["Найдите exact image model и её protocol production-ключом.", "Оцените input бесплатно: countTokens для gemini-3.1-flash-image у Nano Banana 2 или один bounded real request с terminal usage для GPT Image 2.", "Ограничьте форму запроса: output size (1K/2K/4K или auto), число references и maximum attempts на ассет.", "Записывайте terminal usage, request ID и discounted charge каждого settled attempt и сверяйте их с ledger entry в дашборде.", "Измерьте acceptance rate на representative set, умножьте cost per accepted asset на прогнозный объём и задайте lifetime spending limit ключа ниже этого бюджета с alert до исчерпания."],
            ["使用生产密钥发现准确的图像模型及其协议。", "免费估算输入：Nano Banana 2 用 gemini-3.1-flash-image 的 countTokens，GPT Image 2 用一次带终态 usage 的有界真实请求。", "限制请求形态：输出尺寸（1K/2K/4K 或 auto）、参考图数量与每资产最大尝试次数。", "记录每次已结算尝试的终态 usage、request ID 与折后费用，并与仪表板账本记录核对。", "在代表性集合上测量验收率，用每个验收资产成本乘以预测数量，并把密钥终身消费上限设在该预算之下、耗尽前告警。"],
            ["production key로 exact image model과 protocol을 discovery합니다.", "input을 무료로 추정합니다: Nano Banana 2는 gemini-3.1-flash-image의 countTokens, GPT Image 2는 terminal usage가 있는 bounded real request 한 번.", "request 형태를 제한합니다: output size(1K/2K/4K 또는 auto), reference 수, asset당 maximum attempts.", "모든 settled attempt의 terminal usage, request ID, discounted charge를 기록하고 dashboard ledger entry와 대조합니다.", "representative set에서 acceptance rate를 측정해 accepted asset당 비용에 예상 물량을 곱하고 key 평생 누적 지출 한도를 그 예산 아래로 두며 소진 전 alert합니다."],
          ),
          list(
            ["Default to 1K and promote only assets that fail a delivery-resolution check: after the discount the 4K image leg ($0.0756) costs 2.25× the 1K leg ($0.0336).", "Send only references that constrain the result — every reference is billable image input.", "Replace endless prompt retries with a fixed visual checklist and a hard attempt budget per asset.", "Do not count on caching for Nano Banana 2 input: this image model bills cached input at the full rate, while GPT Image 2 cached input settles at 25% of fresh.", "Give each image workload its own key and lifetime spending limit so one campaign cannot consume the whole account balance."],
            ["Начинайте с 1K и повышайте размер только ассетам, не прошедшим delivery-resolution check: после скидки image leg 4K ($0.0756) стоит в 2.25 раза дороже 1K ($0.0336).", "Отправляйте только references, которые ограничивают результат, — каждая reference это billable image input.", "Замените бесконечные prompt retries фиксированным visual checklist и жёстким attempt budget на ассет.", "Не рассчитывайте на кэширование input у Nano Banana 2: эта image-модель тарифицирует cached input по полной ставке, а у GPT Image 2 cached input рассчитывается как 25% от fresh.", "Выдайте каждой image-задаче свой ключ с lifetime spending limit, чтобы одна кампания не потратила весь баланс аккаунта."],
            ["默认使用 1K，仅升级未通过交付分辨率检查的资产：折后 4K 图像项（$0.0756）是 1K（$0.0336）的 2.25 倍。", "只发送确实约束结果的参考图——每张参考图都是计费 image input。", "用固定视觉清单和每资产硬性尝试预算替代无尽提示词重试。", "不要指望 Nano Banana 2 的输入缓存：该图像模型 cached input 按全额费率计费，而 GPT Image 2 的 cached input 按 fresh 的 25% 结算。", "为每个图像工作负载配置独立密钥与终身消费上限，避免单个活动耗尽整个账户余额。"],
            ["기본은 1K로 하고 delivery-resolution check를 통과하지 못한 asset만 올립니다. 할인 후 4K image leg($0.0756)는 1K leg($0.0336)의 2.25배입니다.", "결과를 제약하는 reference만 본냅니다. 모든 reference는 billable image input입니다.", "끝없는 prompt retry 대신 고정 visual checklist와 asset당 엄격한 attempt budget을 사용합니다.", "Nano Banana 2 input cache를 기대하지 마세요. 이 image model은 cached input을 전액 과금하고 GPT Image 2 cached input은 fresh의 25%로 정산됩니다.", "각 image workload에 전용 key와 평생 누적 지출 한도를 줘서 한 캠페인이 전체 계정 잔액을 쓰지 못하게 합니다."],
          ),
        ],
      ),
    ],
    faq: [
      faq(
        tr("What is the cheapest way to estimate an image request?", "Как дешевле всего оценить image request?", "估算图像请求最便宜的方法是什么？", "image request를 가장 저렴하게 추정하는 방법은?"),
        tr("For Nano Banana 2, countTokens estimates input on gemini-3.1-flash-image without generating an image; add the fixed output leg for 1K, 2K or 4K and you have a full pre-generation price. GPT Image 2 has no equivalent estimator, so an authoritative total requires one bounded real request and its terminal usage.", "Для Nano Banana 2 countTokens бесплатно оценивает input для gemini-3.1-flash-image без генерации изображения; добавьте фиксированный output leg для 1K, 2K или 4K — и полная цена известна до генерации. У GPT Image 2 такого estimator нет, поэтому authoritative total требует одного bounded real request и его terminal usage.", "Nano Banana 2 可用 countTokens 免费估算 gemini-3.1-flash-image 的输入而不生成图像，再加 1K、2K 或 4K 的固定输出项，即可在生成前得到完整价格。GPT Image 2 没有等价估算器，权威总额需要一次有界真实请求及其终态 usage。", "Nano Banana 2는 countTokens로 gemini-3.1-flash-image input을 이미지 생성 없이 추정하고 1K, 2K, 4K 고정 output leg를 더하면 생성 전 전체 가격이 나옵니다. GPT Image 2에는 동등한 estimator가 없어 authoritative total에는 bounded real request 한 번과 terminal usage가 필요합니다."),
      ),
      faq(
        tr("Does 50% off mean every picture costs half a fixed list price?", "Означает ли 50%, что каждая картинка стоит половину fixed list price?", "五折是否意味着每张图片都是固定标价的一半？", "50% 할인은 모든 이미지가 고정 가격의 절반이라는 뜻인가요?"),
        tr("No. The policy halves the exact official usage cost for regular B2C. Nano Banana 2 has a predictable image leg by size — $0.0336, $0.0504 and $0.0756 for 1K, 2K and 4K — while GPT Image 2 keeps variable terminal token usage, so only its input rates, not a per-picture total, can be stated in advance.", "Нет. Политика делит exact official usage cost обычного B2C пополам. У Nano Banana 2 предсказуемый image leg по size — $0.0336, $0.0504 и $0.0756 для 1K, 2K и 4K, — а GPT Image 2 сохраняет variable terminal token usage, поэтому заранее можно назвать только его input rates, а не итог за картинку.", "不是。该策略把普通 B2C 的准确官方 usage 成本减半。Nano Banana 2 的图像项按尺寸可预测——1K、2K、4K 分别为 $0.0336、$0.0504、$0.0756；GPT Image 2 仍是可变的终态 token usage，因此只能提前给出输入费率，而非单张总价。", "아닙니다. 일반 B2C의 exact official usage cost를 절반으로 줄입니다. Nano Banana 2는 크기별 image leg가 예측 가능하고(1K $0.0336, 2K $0.0504, 4K $0.0756) GPT Image 2는 terminal token usage가 가변이라 사전에 말할 수 있는 것은 input rate뿐이며 이미지당 총액은 아닙니다."),
      ),
      faq(
        tr("Should failed images be included in the budget?", "Нужно ли учитывать неудачные картинки в бюджете?", "预算是否要计入失败图像？", "실패 이미지도 예산에 포함해야 하나요?"),
        tr("Yes. If a request delivered output and settled usage, its charge is part of acquisition cost even when the asset fails your quality check. Divide total settled spend by accepted assets — that ratio, not the rate card, is what a campaign actually costs.", "Да. Если request доставил output и settled usage, его charge входит в acquisition cost, даже если ассет не прошёл quality check. Делите total settled spend на принятые ассеты — именно это отношение, а не rate card, показывает реальную цену кампании.", "要。如果请求已交付输出并结算 usage，即使资产未通过质量检查，其费用也属于获取成本。用总结算支出除以验收资产数——这个比率而不是费率表，才是活动的真实成本。", "예. request가 output을 전달하고 usage가 정산됐다면 asset이 quality check를 통과하지 못해도 charge는 acquisition cost에 포함됩니다. total settled spend를 accepted asset 수로 나눈 비율이 rate card가 아닌 캠페인의 실제 비용입니다."),
      ),
      faq(
        tr("Where do I verify the final image charge?", "Где проверить итоговое списание за image?", "在哪里核对最终图像扣费？", "최종 image charge는 어디서 확인하나요?"),
        tr("Use terminal provider usage together with the matching dashboard ledger entry for that request ID. Do not infer money from PNG file size, pixel dimensions or partial output — none of them is the billing formula.", "Сверьте terminal provider usage с matching ledger entry в дашборде по request ID. Не выводите сумму из размера PNG, pixel dimensions или partial output — ни один из них не является формулой billing.", "按 request ID 将提供商终态 usage 与匹配的仪表板账本记录一起核对；不要从 PNG 文件大小、像素尺寸或部分输出推断金额——它们都不是计费公式。", "request ID의 terminal provider usage와 matching dashboard ledger entry를 함께 확인하고 PNG 파일 크기, pixel dimensions, partial output으로 금액을 추론하지 마세요. 어느 것도 billing 공식이 아닙니다."),
      ),
      faq(
        tr("Can I test image generation pricing without spending money?", "Можно ли проверить цены image generation без трат?", "能否不花钱测试图像生成定价？", "돈을 쓰지 않고 image generation 가격을 테스트할 수 있나요?"),
        tr("Mostly yes. Accounts created with Google or GitHub receive a $5 platform bonus credit that is spent before any paid balance, and countTokens estimates Nano Banana 2 input for free. The only step that always costs is the first real generation — keep it bounded at 1K or quality low and cap the key with a lifetime spending limit.", "В основном да. Аккаунты, созданные через Google или GitHub, получают platform bonus credit $5, который тратится раньше paid balance, а countTokens бесплатно оценивает input Nano Banana 2. Единственный всегда платный шаг — первая реальная генерация: держите её bounded на 1K или quality low и ограничьте ключ lifetime spending limit.", "基本可以。通过 Google 或 GitHub 创建的账户获得 $5 平台赠金，先于付费余额消耗；countTokens 可免费估算 Nano Banana 2 输入。唯一必然付费的步骤是首次真实生成——将其限制在 1K 或 quality low，并为密钥设置终身消费上限。", "대체로 가능합니다. Google이나 GitHub로 만든 계정은 paid balance보다 먼저 쓰이는 $5 platform bonus credit을 받고 countTokens는 Nano Banana 2 input을 묣료로 추정합니다. 항상 비용이 드는 유일한 단계는 첫 실제 생성이므로 1K나 quality low로 제한하고 key에 평생 누적 지출 한도를 두세요."),
      ),
    ],
  };
