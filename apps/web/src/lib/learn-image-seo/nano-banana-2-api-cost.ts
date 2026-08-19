import type { ImageSeoSpec } from "./shared";
import { faq, list, note, paragraph, section, sharedCode, steps, table, tr, ROUTER } from "./shared";

export const spec: ImageSeoSpec = {
  slug: "nano-banana-2-api-cost",
  cluster: "explain",
  related: ["nano-banana-2-api-guide", "gpt-image-2-api-cost", "image-generation-api-pricing", "gemini-api-pricing"],
  title: tr(
    "Nano Banana 2 API Cost: Exact Prices, 50% Off",
    "Стоимость Nano Banana 2 API: скидка 50% для B2C",
    "Nano Banana 2 API 成本：准确价格与 B2C 五折优惠",
    "Nano Banana 2 API 비용: 정확한 가격과 B2C 50% 절감",
  ),
  h1: tr(
    "Nano Banana 2 API cost and 50% B2C savings",
    "Стоимость Nano Banana 2 API и экономия 50% для B2C",
    "Nano Banana 2 API 成本与 B2C 五折优惠",
    "Nano Banana 2 API 비용과 B2C 50% 할인",
  ),
  description: tr(
    "Nano Banana 2 API cost by size: exact image-token rates for 1K, 2K and 4K, a flat 50% B2C discount, worked batch math and budgeting safeguards.",
    "Стоимость Nano Banana 2 API: точные ставки image tokens для 1K, 2K и 4K, скидка 50% для B2C, готовый расчёт пакета и контроль бюджета.",
    "计算 Nano Banana 2 的 1K、2K、4K 图像生成成本：准确 image token、apiToken.sale B2C 五折价格、完整批次演算与预算保护。",
    "Nano Banana 2의 1K, 2K, 4K 이미지 생성 비용, 정확한 image token, apiToken.sale B2C 50% 가격, 실제 배치 계산과 예산 보호를 확인하세요.",
  ),
  keywords: tr(
    ["nano banana 2 api cost", "nano banana 2 price", "cheap nano banana api", "gemini image generation cost", "nano banana 2 discount", "gemini 3.1 flash image pricing"],
    ["nano banana 2 цена api", "стоимость nano banana 2", "дешевый nano banana api", "цена генерации gemini", "скидка nano banana 2", "gemini 3.1 flash image цена"],
    ["nano banana 2 api 成本", "nano banana 2 价格", "便宜 nano banana api", "gemini 图像生成成本", "nano banana 2 折扣", "gemini 3.1 flash image 定价"],
    ["nano banana 2 api 비용", "nano banana 2 가격", "저렴한 nano banana api", "gemini 이미지 생성 비용", "nano banana 2 할인", "gemini 3.1 flash image 가격"],
  ),
  dek: tr(
    "Nano Banana 2 bills a fixed, published number of image-output tokens for each live size — 1K, 2K and 4K — and a regular B2C account pays half of that official image leg. Input, text/thinking output and grounding remain separate legs, reported in the terminal usage of every response.",
    "У Nano Banana 2 для каждого доступного размера — 1K, 2K и 4K — зафиксировано опубликованное число image-output tokens, и обычный B2C-аккаунт платит половину официальной image-составляющей. Input, text/thinking output и grounding считаются отдельно и видны в terminal usage каждого ответа.",
    "Nano Banana 2 对每个可用尺寸（1K、2K、4K）按固定且公开的 image-output token 数量计费，普通 B2C 账户只需支付官方图像费用的一半。输入、文本/思考输出与 grounding 仍单独计费，并在每次响应的终态 usage 中列明。",
    "Nano Banana 2는 제공되는 각 크기(1K, 2K, 4K)에 고정된 공개 image-output token 수를 과금하며, 일반 B2C 계정은 공식 이미지 leg의 절반만 냅니다. input, text/thinking output, grounding은 별도 leg로 매 응답의 terminal usage에 보고됩니다.",
  ),
  sections: [
    section(
      tr(
        "Exact image-output prices at 1K, 2K and 4K",
        "Точные цены image output для 1K, 2K и 4K",
        "1K、2K、4K 图像输出的准确价格",
        "1K, 2K, 4K image output의 정확한 가격",
      ),
      [
        paragraph(
          "A 1K Nano Banana 2 image costs a regular B2C account $0.0336 in image-output tokens on apiToken.sale — exactly half of the official $0.0672. The 2K and 4K sizes come to $0.0504 and $0.0756 under the same flat 50% discount. Each size bills a fixed, published number of image tokens at an official $60 per million, so the output leg of every request is predictable before you send it. What stays variable is everything else the model meters: text and reference-image input, any text or thinking the model writes back, and grounding — each reported separately in the terminal usage of the response.",
          "Одно изображение Nano Banana 2 в размере 1K обходится обычному B2C-аккаунту на apiToken.sale в $0.0336 image-output tokens — ровно половина официальных $0.0672. Размеры 2K и 4K при той же фиксированной скидке 50% стоят $0.0504 и $0.0756. Каждый размер тарифицируется фиксированным опубликованным числом image tokens по официальной ставке $60 за миллион, поэтому output-составляющая запроса известна ещё до отправки. Переменной остаётся остальная часть счёта: text и reference-image input, возможный text/thinking output и grounding — каждая позиция отдельно видна в terminal usage ответа.",
          "在 apiToken.sale 上，普通 B2C 账户生成一张 1K Nano Banana 2 图像的 image-output token 费用为 $0.0336，恰好是官方 $0.0672 的一半；2K 和 4K 在同样五折下分别为 $0.0504 和 $0.0756。每个尺寸都按固定且公开的数量、以官方每百万 $60 的费率计 image token，因此请求的输出费用在发送前即可确定。仍属可变的部分是模型计量的其余项目：文本与参考图输入、模型可能返回的文本或思考输出，以及 grounding——每一项都会在响应的终态 usage 中单独列出。",
          "apiToken.sale에서 일반 B2C 계정의 Nano Banana 2 1K 이미지 한 장은 image-output token 기준 $0.0336으로, 공식 $0.0672의 정확히 절반입니다. 같은 고정 50% 할인으로 2K와 4K는 각각 $0.0504, $0.0756입니다. 각 크기는 공식 $60/백만 요율로 고정된 공개 image token 수를 과금하므로 요청의 output leg는 보내기 전에 확정됩니다. 가변으로 남는 부분은 모델이 계량하는 나머지 항목, 즉 텍스트와 reference image input, 모델이 함께 반환하는 text/thinking output, grounding이며 각각 응답의 terminal usage에 따로 보고됩니다.",
        ),
        table(
          { headers: ["Size", "Billable image tokens", "Official image output", "Regular B2C here"], rows: [["1K", "1,120", "$0.0672", "$0.0336"], ["2K", "1,680", "$0.1008", "$0.0504"], ["4K", "2,520", "$0.1512", "$0.0756"]] },
          { headers: ["Размер", "Image tokens", "Официальный image output", "Обычный B2C здесь"], rows: [["1K", "1 120", "$0.0672", "$0.0336"], ["2K", "1 680", "$0.1008", "$0.0504"], ["4K", "2 520", "$0.1512", "$0.0756"]] },
          { headers: ["尺寸", "计费 image token", "官方图像输出", "本站普通 B2C"], rows: [["1K", "1,120", "$0.0672", "$0.0336"], ["2K", "1,680", "$0.1008", "$0.0504"], ["4K", "2,520", "$0.1512", "$0.0756"]] },
          { headers: ["크기", "과금 image token", "공식 이미지 output", "일반 B2C 가격"], rows: [["1K", "1,120", "$0.0672", "$0.0336"], ["2K", "1,680", "$0.1008", "$0.0504"], ["4K", "2,520", "$0.1512", "$0.0756"]] },
        ),
        note(
          "These are image-output charges, not a promise that the whole request costs exactly that amount. Add text/image input, any text or thinking output, and grounding reported by terminal usage.",
          "Это цена image output, а не обещание полной цены запроса. Добавьте text/image input, возможный text/thinking output и grounding из terminal usage.",
          "这些是图像输出费用，并非整个请求的固定总价；还需加上 text/image input、文本或思考输出以及终态 usage 中的 grounding。",
          "이는 image output 비용이며 전체 요청의 고정 가격이 아닙니다. text/image input, text/thinking output, terminal usage의 grounding을 더해야 합니다.",
        ),
        tr(
          { type: "link", text: "Nano Banana 2 model page: rates, limits and availability", href: "/models/gemini-3-1-flash-image" },
          { type: "link", text: "Страница модели Nano Banana 2: ставки, лимиты и доступность", href: "/models/gemini-3-1-flash-image" },
          { type: "link", text: "Nano Banana 2 模型页：费率、限额与可用性", href: "/models/gemini-3-1-flash-image" },
          { type: "link", text: "Nano Banana 2 모델 페이지: 요금, 제한, 가용성", href: "/models/gemini-3-1-flash-image" },
        ),
      ],
    ),
    section(
      tr(
        "The other legs on a Nano Banana 2 bill",
        "Остальные составляющие счёта Nano Banana 2",
        "Nano Banana 2 账单上的其他计费项",
        "Nano Banana 2 청구서의 나머지 과금 항목",
      ),
      [
        paragraph(
          "Nano Banana 2 is served as gemini-3.1-flash-image and, like every Gemini model, it meters input and output separately. Text input is $0.50 per million tokens officially; reference images you upload for editing are converted into input tokens at the same model rate; image output is the fixed $60 per million with the per-size counts from the table above. If the model also returns text — a caption, an explanation, thinking — that is a separate output leg, and grounding, when enabled, is metered on its own. The terminal usage of each response is the authoritative record of all these legs, and the 50% B2C discount applies to the exact official total, not only to the image.",
          "Nano Banana 2 доступен как gemini-3.1-flash-image и, как любая модель Gemini, отдельно считает input и output. Text input стоит официально $0.50 за миллион tokens; загруженные для редактирования reference-изображения превращаются в input tokens по той же ставке модели; image output — фиксированные $60 за миллион с показанными выше counts по размерам. Если модель дополнительно возвращает текст — подпись, пояснение, thinking, — это отдельная output-составляющая, а включённый grounding тарифицируется сам по себе. Terminal usage каждого ответа — авторитетная запись всех этих составляющих, и скидка 50% для B2C применяется к точному официальному итогу, а не только к изображению.",
          "Nano Banana 2 以 gemini-3.1-flash-image 提供，与所有 Gemini 模型一样分别计量输入与输出。文本输入官方费率为每百万 token $0.50；为编辑上传的参考图按同一模型费率折算为 input token；图像输出为固定每百万 $60，各尺寸数量见上表。如果模型同时返回文本——说明、解释或思考——这是独立的输出计费项，启用 grounding 时也单独计量。每次响应的终态 usage 是所有这些计费项的权威记录，B2C 五折适用于准确的官方总额，而不仅仅是图像部分。",
          "Nano Banana 2는 gemini-3.1-flash-image로 제공되며, 모든 Gemini 모델처럼 input과 output을 분리 계량합니다. text input은 공식 1백만 token당 $0.50이고, 편집용으로 업로드하는 reference image는 같은 모델 요율의 input token으로 환산되며, image output은 위 표의 크기별 고정 수량으로 1백만당 $60입니다. 모델이 캡션, 설명, thinking 같은 텍스트를 함께 반환하면 별도의 output leg가 되고, grounding을 켜면 독립적으로 계량됩니다. 각 응답의 terminal usage가 이 모든 leg의 권위 있는 기록이며, B2C 50% 할인은 이미지만이 아니라 정확한 공식 총액에 적용됩니다.",
        ),
        table(
          { headers: ["Usage leg", "Official rate", "Regular B2C"], rows: [["Text input (prompt)", "$0.50 / 1M tokens", "50% off"], ["Reference image input", "input tokens at the model rate", "50% off"], ["Image output", "$60 / 1M tokens, fixed counts by size", "50% off"], ["Cached input", "full input rate — no provider-side cache discount", "50% off that official amount"], ["Text/thinking output, grounding", "as reported in terminal usage", "50% off the official total"]] },
          { headers: ["Составляющая", "Официальная ставка", "Обычный B2C"], rows: [["Text input (prompt)", "$0.50 / 1M tokens", "скидка 50%"], ["Reference image input", "input tokens по ставке модели", "скидка 50%"], ["Image output", "$60 / 1M tokens, фиксированные counts по размеру", "скидка 50%"], ["Cached input", "полная input-ставка, без provider-скидки на cache", "50% от этой официальной суммы"], ["Text/thinking output, grounding", "по данным terminal usage", "50% от официального итога"]] },
          { headers: ["计费项", "官方费率", "普通 B2C"], rows: [["Text input（提示词）", "$0.50 / 1M token", "五折"], ["Reference image input", "按模型费率的 input token", "五折"], ["Image output", "$60 / 1M token，按尺寸固定数量", "五折"], ["Cached input", "按完整 input 费率，无服务商缓存折扣", "该官方金额五折"], ["Text/thinking output、grounding", "以终态 usage 为准", "官方总额五折"]] },
          { headers: ["과금 항목", "공식 요율", "일반 B2C"], rows: [["Text input (prompt)", "$0.50 / 1M token", "50% 할인"], ["Reference image input", "모델 요율의 input token", "50% 할인"], ["Image output", "$60 / 1M token, 크기별 고정 수량", "50% 할인"], ["Cached input", "전체 input 요율 — 제공자 cache 할인 없음", "해당 공식 금액의 50%"], ["Text/thinking output, grounding", "terminal usage 보고 기준", "공식 총액의 50%"]] },
        ),
        paragraph(
          "The counterintuitive row is cached input. Text-oriented Gemini tiers discount repeated prefixes; this image model does not — cached input is billed at the full input rate. A pipeline that re-uploads the same twenty reference photos for every candidate pays for them every single time, so bounding the reference set is a real cost control, not a hygiene preference.",
          "Нетривиальная строка здесь — cached input. Текстовые тарифы Gemini дают скидку на повторные префиксы, а эта image-модель — нет: cached input оплачивается по полной input-ставке. Конвейер, который заново загружает одни и те же двадцать reference-фото для каждого варианта, платит за них каждый раз, поэтому ограничение набора references — реальный контроль расходов, а не вопрос аккуратности.",
          "其中最容易被误解的是 cached input。Gemini 的文本档位会对重复前缀打折，而这个图像模型不会——cached input 按完整 input 费率计费。如果流水线为每个候选图都重新上传同样的二十张参考图，那么每一次都要为这些参考图付费；因此控制参考图集合的规模是实打实的成本手段，而不仅仅是整洁习惯。",
          "직관과 다른 행은 cached input입니다. 텍스트 중심 Gemini 등급은 반복 prefix를 할인하지만 이 image 모델은 그렇지 않습니다 — cached input도 전체 input 요율로 과금됩니다. 모든 candidate마다 같은 reference 사진 20장을 다시 업로드하는 파이프라인은 매번 그 비용을 지불하므로, reference 세트를 제한하는 것은 단순한 정리 습관이 아니라 실제 비용 통제 수단입니다.",
        ),
      ],
    ),
    section(
      tr(
        "Cost math for a real catalog batch",
        "Расчёт стоимости реального пакета для каталога",
        "真实电商目录批次的成本演算",
        "실제 카탈로그 배치 비용 계산",
      ),
      [
        paragraph(
          "Take a typical ecommerce workload: 200 product packshots at 1K, 50 background-swap variants at 2K and 10 hero banners at 4K. At regular B2C prices the image-output legs total $9.996. Prompts move the needle far less than people expect: even a generous 300-token brief for each of the 260 images adds 78,000 input tokens — $0.039 officially, under two cents after the discount. Resolution, not prose, is the budget lever.",
          "Возьмём типичную ecommerce-задачу: 200 карточек товара в 1K, 50 вариантов со сменой фона в 2K и 10 hero-баннеров в 4K. По ценам обычного B2C image-output составляющие дают в сумме $9.996. Промпты влияют на итог гораздо слабее, чем принято думать: даже щедрое техзадание на 300 tokens для каждого из 260 изображений добавляет 78 000 input tokens — $0.039 официально, меньше двух центов после скидки. Рычаг бюджета — разрешение, а не текст.",
          "以一个典型的电商工作负载为例：200 张 1K 产品白底图、50 张 2K 换背景变体、10 张 4K 主视觉横幅。按普通 B2C 价格，图像输出费用合计 $9.996。提示词对总额的影响远小于人们的直觉：即便为 260 张图每张都写 300 token 的详细需求，也只增加 78,000 个 input token——官方价 $0.039，折后不到两美分。真正的预算杠杆是分辨率，而不是文字。",
          "전형적인 ecommerce 작업을 예로 들면: 1K 상품 팩샷 200장, 2K 배경 교체 변형 50장, 4K 히어로 배너 10장입니다. 일반 B2C 가격에서 image-output leg 합계는 $9.996입니다. 프롬프트는 생각보다 총액을 거의 움직이지 않습니다. 260장 각각에 300 token의 상세 지시를 써도 input token 78,000개, 즉 공식 $0.039, 할인 후 2센트 미만이 추가될 뿐입니다. 예산의 지렛대는 문장이 아니라 해상도입니다.",
        ),
        table(
          { headers: ["Workload", "Size", "Images", "Image-output cost (regular B2C)"], rows: [["Product packshots", "1K", "200", "$6.72"], ["Background-swap variants", "2K", "50", "$2.52"], ["Hero banners", "4K", "10", "$0.756"], ["Batch total", "—", "260", "$9.996"]] },
          { headers: ["Задача", "Размер", "Изображений", "Цена image output (обычный B2C)"], rows: [["Карточки товара", "1K", "200", "$6.72"], ["Варианты со сменой фона", "2K", "50", "$2.52"], ["Hero-баннеры", "4K", "10", "$0.756"], ["Итого за пакет", "—", "260", "$9.996"]] },
          { headers: ["工作负载", "尺寸", "数量", "图像输出费用（普通 B2C）"], rows: [["产品白底图", "1K", "200", "$6.72"], ["换背景变体", "2K", "50", "$2.52"], ["主视觉横幅", "4K", "10", "$0.756"], ["批次合计", "—", "260", "$9.996"]] },
          { headers: ["작업", "크기", "장수", "Image-output 비용 (일반 B2C)"], rows: [["상품 팩샷", "1K", "200", "$6.72"], ["배경 교체 변형", "2K", "50", "$2.52"], ["히어로 배너", "4K", "10", "$0.756"], ["배치 합계", "—", "260", "$9.996"]] },
        ),
        paragraph(
          "Run the sensitivity check before committing to 4K anywhere: rendering all 260 assets at 4K would cost $39.312 in image output alone — almost four times the mixed batch. The policy most teams land on is to default everything to 1K and promote only the assets that fail a delivery-resolution check at their actual placement.",
          "Прежде чем где-либо фиксировать 4K, посчитайте чувствительность: рендер всех 260 ассетов в 4K стоил бы $39.312 одним лишь image output — почти вчетверо дороже смешанного пакета. Практика, к которой приходят большинство команд: по умолчанию всё в 1K, а повышать размер только для ассетов, не прошедших проверку разрешения в реальном месте публикации.",
          "在任何环节锁定 4K 之前，先做敏感性检查：如果把 260 个资产全部按 4K 渲染，仅图像输出就要 $39.312——接近混合批次的四倍。多数团队最终采用的策略是：全部默认 1K，只有未通过实际投放位置交付分辨率检查的资产才升级尺寸。",
          "어디든 4K를 고정하기 전에 민감도 점검을 하세요. 260개 asset을 모두 4K로 렌더링하면 image output만 $39.312로 혼합 배치의 거의 네 배입니다. 대부분의 팀이 도달하는 정책은 모두 1K를 기본으로 하고, 실제 게재 위치의 배포 해상도 검사를 통과하지 못한 asset만 승격하는 것입니다.",
        ),
        tr(
          { type: "link", text: "Full image generation API pricing: every usage leg explained", href: "/docs/learn/image-generation-api-pricing" },
          { type: "link", text: "Полный разбор цен image generation API: все составляющие usage", href: "/docs/learn/image-generation-api-pricing" },
          { type: "link", text: "图像生成 API 完整定价：逐项解析全部 usage 计费项", href: "/docs/learn/image-generation-api-pricing" },
          { type: "link", text: "이미지 생성 API 전체 가격: 모든 usage leg 상세 설명", href: "/docs/learn/image-generation-api-pricing" },
        ),
      ],
    ),
    section(
      tr(
        "Estimate with countTokens, generate at an explicit size",
        "Оценка через countTokens, генерация с явным размером",
        "先用 countTokens 估算，再按明确尺寸生成",
        "countTokens로 추정하고 명시적 크기로 생성",
      ),
      [
        steps(
          ["Call countTokens on gemini-3.1-flash-image to meter the prompt and any reference images without buying an image.", "Choose 1K, 2K or 4K explicitly in imageConfig; do not reserve 4K for assets that ship at 1K.", "Add the fixed image-output leg from the table above to the bounded input and any planned grounding.", "Set a lifetime spending limit on the image-workload key and reconcile the terminal usage of the first real call against your estimate.", "Read the image from the inlineData base64 payload and store it yourself; treat usageMetadata as the billing record, not the estimate."],
          ["Вызовите countTokens для gemini-3.1-flash-image, чтобы бесплатно измерить промпт и reference-изображения, не покупая картинку.", "Явно выберите 1K, 2K или 4K в imageConfig; не резервируйте 4K для ассета, который будет опубликован в 1K.", "Прибавьте фиксированную image-output составляющую из таблицы к ограниченному input и запланированному grounding.", "Задайте ключу image-нагрузки общий лимит расходов и сверьте terminal usage первого реального вызова с оценкой.", "Заберите изображение из base64-поля inlineData и сохраните его самостоятельно; billing-записью считайте usageMetadata, а не оценку."],
          ["对 gemini-3.1-flash-image 调用 countTokens，免费计量提示词与参考图，无需购买图像。", "在 imageConfig 中明确选择 1K、2K 或 4K；最终只发布 1K 的资产不要预留 4K。", "把上表中的固定图像输出费用加到有界输入与计划使用的 grounding 上。", "为图像工作负载密钥设置终身消费上限，并把首个真实请求的终态 usage 与估算值核对。", "从 inlineData 的 base64 载荷中取出图像并自行存储；以 usageMetadata 作为计费记录，而不是估算值。"],
          ["gemini-3.1-flash-image에 countTokens를 호출해 이미지를 사지 않고 프롬프트와 reference image를 계량합니다.", "imageConfig에서 1K, 2K, 4K를 명시하고 1K로 배포할 asset에 4K를 예약하지 않습니다.", "위 표의 고정 image-output leg를 제한된 input과 계획한 grounding에 더합니다.", "이미지 workload key에 평생 누적 지출 한도를 설정하고 첫 실제 호출의 terminal usage를 추정치와 대조합니다.", "inlineData base64 페이로드에서 이미지를 읽어 직접 저장하고, 추정치가 아니라 usageMetadata를 청구 기록으로 취급합니다."],
        ),
        sharedCode(`curl ${ROUTER}/v1beta/models/gemini-3.1-flash-image:generateContent \\
  -H "x-goog-api-key: $APITOKEN_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{"contents":[{"parts":[{"text":"Create a square product illustration"}]}],"generationConfig":{"responseModalities":["TEXT","IMAGE"],"imageConfig":{"imageSize":"1K","aspectRatio":"1:1"}}}'`),
        paragraph(
          "The response carries the image inline: a base64 payload with its mimeType inside candidates[0].content.parts, not a URL to fetch later — plan your storage accordingly. The usageMetadata block of the same response is the terminal, authoritative usage record: the fixed image-token count of the render appears there alongside the prompt and any text-output counts. Reconciling that field with your countTokens estimate after the first call is the cheapest audit you will ever run.",
          "Ответ несёт изображение внутри себя: base64-полезная нагрузка с mimeType в candidates[0].content.parts, а не URL для последующей загрузки — планируйте хранение с учётом этого. Блок usageMetadata того же ответа — терминальная, авторитетная запись usage: фиксированное число image tokens рендера фигурирует там рядом с prompt и text-output счётчиками. Сверка этого поля с оценкой countTokens после первого вызова — самый дешёвый аудит из возможных.",
          "响应内联携带图像：candidates[0].content.parts 中是带 mimeType 的 base64 载荷，而不是稍后抓取的 URL——请据此规划存储。同一响应中的 usageMetadata 块是终态、权威的 usage 记录：渲染的固定 image token 数量与 prompt、文本输出计数一起列在其中。在首次调用后把该字段与 countTokens 的估算核对，是成本最低的对账方式。",
          "응답은 이미지를 인라인으로 담고 있습니다. 나중에 가져올 URL이 아니라 candidates[0].content.parts 안에 mimeType과 함께 있는 base64 페이로드이므로 저장을 그에 맞게 계획하세요. 같은 응답의 usageMetadata 블록이 최종적이고 권위 있는 usage 기록으로, 렌더의 고정 image token 수가 prompt 및 text output 카운트와 함께 나타납니다. 첫 호출 후 이 필드를 countTokens 추정치와 대조하는 것이 가장 저렴한 감사입니다.",
        ),
        sharedCode(`{
  "candidates": [
    {
      "content": {
        "role": "model",
        "parts": [
          { "text": "Here is the illustration." },
          { "inlineData": { "mimeType": "image/png", "data": "iVBORw0KGgo..." } }
        ]
      },
      "finishReason": "STOP"
    }
  ],
  "usageMetadata": {
    "promptTokenCount": 24,
    "candidatesTokenCount": 1144,
    "totalTokenCount": 1168
  }
}`),
      ],
    ),
    section(
      tr(
        "Where the 50% saving applies — and where it stops",
        "Где действует экономия 50% — и где она заканчивается",
        "五折优惠的适用范围与边界",
        "50% 절감이 적용되는 범위와 한계",
      ),
      [
        paragraph(
          "The 50% figure is the regular B2C policy applied after official usage is calculated: the router meters the exact official total of every leg and bills half of it. B2B accounts follow their negotiated policy, and OpenKeys stay at official 1:1 prices. A discount also never proves availability — whether gemini-3.1-flash-image currently serves a particular key is a property of the live route, not of the price list.",
          "50% — это политика обычного B2C, применяемая после расчёта official usage: роутер измеряет точный официальный итог по всем составляющим и списывает его половину. У B2B-аккаунтов действует согласованная политика, а OpenKeys остаются на официальных ценах 1:1. Скидка также ничего не говорит о доступности: обслуживает ли gemini-3.1-flash-image конкретный ключ прямо сейчас — свойство живого маршрута, а не прайс-листа.",
          "50% 是在官方 usage 计算完成后应用的普通 B2C 策略：路由层计量每一项的准确官方总额，然后按一半扣费。B2B 账户遵循协商策略，OpenKeys 保持官方 1:1 价格。折扣同样不代表可用性——gemini-3.1-flash-image 当前是否服务于某个密钥，取决于实时路由，而不是价格表。",
          "50%는 official usage 계산 뒤 적용되는 일반 B2C 정책으로, 라우터가 모든 leg의 정확한 공식 총액을 계량하고 그 절반을 청구합니다. B2B 계정은 협상 정책을 따르고 OpenKeys는 공식 1:1 가격을 유지합니다. 할인은 가용성을 증명하지도 않습니다 — gemini-3.1-flash-image가 특정 key를 현재 서빙하는지는 가격표가 아니라 라이브 route의 속성입니다.",
        ),
        list(
          ["Use a text Flash model for prompt rewriting and classification; call Flash Image only when the response must contain pixels.", "Default to 1K and promote only assets that fail a delivery-resolution check at their real placement.", "Reuse one bounded reference set instead of re-uploading a large collection with every candidate.", "Do not budget for a cache discount: this image model bills cached input at the full input rate.", "Keep 0.5K out of planning: the live subscription route currently admits only 1K, 2K and 4K."],
          ["Переписывайте prompt и классифицируйте через text Flash; вызывайте Flash Image только когда ответ должен содержать пиксели.", "По умолчанию работайте в 1K и повышайте размер только для ассетов, не прошедших проверку разрешения в реальном месте публикации.", "Используйте один ограниченный набор references вместо повторной загрузки большой коллекции в каждый вариант.", "Не закладывайте в бюджет скидку на cache: cached input этой image-модели оплачивается по полной input-ставке.", "Не планируйте 0.5K: текущий subscription route допускает только 1K, 2K и 4K."],
          ["提示词改写与分类交给文本 Flash 模型；只有响应必须包含图像时才调用 Flash Image。", "默认使用 1K，仅对未通过实际投放位置交付分辨率检查的资产升级。", "复用同一个有界参考图集合，不要为每个候选项重复上传大型集合。", "预算中不要计入缓存折扣：该图像模型的 cached input 按完整 input 费率计价。", "规划时不要考虑 0.5K：当前订阅路由仅允许 1K、2K 和 4K。"],
          ["prompt 재작성과 분류는 text Flash 모델로 처리하고, 응답에 픽셀이 필요할 때만 Flash Image를 호출합니다.", "기본은 1K로 하고 실제 게재 위치의 배포 해상도 검사를 통과하지 못한 asset만 승격합니다.", "모든 candidate에 큰 컬렉션을 다시 올리지 말고 제한된 reference 세트 하나를 재사용합니다.", "예산에 cache 할인을 넣지 마세요. 이 image 모델은 cached input도 전체 input 요율로 청구합니다.", "0.5K는 계획에서 제외하세요. 현재 subscription route는 1K, 2K, 4K만 허용합니다."],
        ),
        note(
          "Funding is prepaid: top up any whole-dollar amount by bank card or crypto (USDT, BTC and other major coins) through the secure checkout provider, and the balance never expires. New accounts created with Google or GitHub start with $5 of platform bonus credit — valid on supported Claude, GPT, Gemini and Kimi models and always spent before paid balance — which covers roughly 148 discounted 1K image-output legs before any top-up. Per-key guardrails are the ones that exist: a lifetime spending limit and an expiration date.",
          "Пополнение — предоплатное: любую сумму в целых долларах можно внести банковской картой или криптой (USDT, BTC и другие основные монеты) через защищённого checkout-провайдера, и баланс не сгорает. Новые аккаунты через Google или GitHub начинают с бонуса $5 на баланс платформы — он действует на поддерживаемые модели Claude, GPT, Gemini и Kimi и всегда тратится раньше оплаченного баланса — а это примерно 148 image-output составляющих 1K со скидкой ещё до первого пополнения. Из guardrails на ключ существуют ровно два: общий лимит расходов и дата истечения.",
          "账户为预付制：可通过安全的收银服务商用银行卡或加密货币（USDT、BTC 及其他主流币种）充值任意整数美元，余额永不过期。通过 Google 或 GitHub 注册的新账户可获得 $5 平台赠送额度——适用于支持的 Claude、GPT、Gemini 和 Kimi 模型，且总是先于付费余额消耗——按折扣价计算约可覆盖 148 次 1K 图像输出。每个密钥可用的防护措施就是实际存在的这两项：终身消费上限与到期日期。",
          "충전은 선불입니다. 안전한 checkout 제공업체를 통해 은행 카드 또는 암호화폐(USDT, BTC 및 기타 주요 코인)로 정수 달러 단위로 충전하며 잔액은 만료되지 않습니다. Google 또는 GitHub로 만든 신규 계정은 $5 플랫폼 보너스 크레딧으로 시작하며, 지원되는 Claude, GPT, Gemini, Kimi 모델에 사용할 수 있고 항상 유료 잔액보다 먼저 소진됩니다. 할인가 기준 1K image-output leg 약 148회 분량을 첫 충전 전에 쓸 수 있습니다. key별 guardrail은 실제로 존재하는 두 가지, 평생 누적 지출 한도와 만료일입니다.",
        ),
      ],
    ),
  ],
  faq: [
    faq(
      tr("How much does one 1K Nano Banana 2 image cost on apiToken.sale?", "Сколько стоит одно изображение Nano Banana 2 в 1K на apiToken.sale?", "在 apiToken.sale 上一张 1K Nano Banana 2 图像多少钱？", "apiToken.sale에서 Nano Banana 2 1K 이미지 한 장은 얼마인가요?"),
      tr(
        "The fixed image-output leg is $0.0336 for a regular B2C account — the official $0.0672 (1,120 image tokens at $60 per million) cut in half. Text and reference-image input, optional text or thinking output, and grounding are metered separately and reported in the response's terminal usage.",
        "Фиксированная image-output составляющая — $0.0336 для обычного B2C: официальные $0.0672 (1 120 image tokens по $60 за миллион), поделённые пополам. Text и reference-image input, возможный text/thinking output и grounding тарифицируются отдельно и видны в terminal usage ответа.",
        "普通 B2C 账户的固定图像输出费用为 $0.0336——官方 $0.0672（1,120 个 image token，每百万 $60）的一半。文本与参考图输入、可选的文本或思考输出以及 grounding 单独计费，并在响应的终态 usage 中列明。",
        "일반 B2C 계정의 고정 image-output leg는 $0.0336으로, 공식 $0.0672(1,120 image token, 1백만당 $60)의 절반입니다. text와 reference image input, 선택적 text/thinking output, grounding은 별도 계량되어 응답의 terminal usage에 보고됩니다.",
      ),
    ),
    faq(
      tr("Is 4K always four times the 1K price?", "Всегда ли 4K стоит в четыре раза дороже 1K?", "4K 总是 1K 价格的四倍吗？", "4K는 항상 1K 가격의 네 배인가요?"),
      tr(
        "No. The published image-output legs are $0.0672 official for 1K and $0.1512 for 4K — a 2.25x step, because the fixed token counts scale 1,120 to 2,520 rather than with the pixel count. Use the exact size table instead of multiplying dimensions.",
        "Нет. Официальные image-output составляющие — $0.0672 для 1K и $0.1512 для 4K, то есть шаг 2.25x: фиксированные token counts растут с 1 120 до 2 520, а не пропорционально числу пикселей. Пользуйтесь точной таблицей размеров, а не умножением сторон.",
        "不是。官方公布的图像输出费用为 1K $0.0672、4K $0.1512——只有 2.25 倍，因为固定 token 数量是从 1,120 增至 2,520，而不是随像素数同比增长。请使用准确的尺寸表，而不是按边长倍数推算。",
        "아닙니다. 공식 image-output leg는 1K $0.0672, 4K $0.1512로 2.25배 단계입니다. 고정 token 수가 픽셀 수에 비례하지 않고 1,120에서 2,520으로 증가하기 때문입니다. 크기를 곱하지 말고 정확한 크기 표를 사용하세요.",
      ),
    ),
    faq(
      tr("Does the Nano Banana 2 discount apply to every account?", "Скидка Nano Banana 2 действует на любой аккаунт?", "Nano Banana 2 折扣适用于所有账户吗？", "Nano Banana 2 할인이 모든 계정에 적용되나요?"),
      tr(
        "The flat 50% policy applies to regular B2C accounts. B2B pricing follows the account's negotiated rules, while OpenKeys bill at official 1:1 prices.",
        "Фиксированные 50% действуют для обычных B2C. Цена B2B определяется индивидуальными правилами аккаунта, а OpenKeys тарифицируются 1:1 по официальной цене.",
        "固定五折适用于普通 B2C 账户；B2B 按账户协商规则计价，OpenKeys 则按官方价格 1:1 计费。",
        "고정 50%는 일반 B2C 계정에 적용됩니다. B2B는 계정별 협상 규칙을 따르고, OpenKeys는 공식 가격 1:1로 과금됩니다.",
      ),
    ),
    faq(
      tr("Can I drop to 0.5K to save even more?", "Можно ли перейти на 0.5K и сэкономить ещё больше?", "能否降到 0.5K 进一步省钱？", "0.5K로 낮춰 더 절약할 수 있나요?"),
      tr(
        "Not on the live subscription route. It currently admits 1K, 2K and 4K; 0.5K is rejected locally until that private capability is live-verified. The cheapest size you can actually budget for today is 1K at $0.0336 per image output.",
        "Не на текущем subscription route. Сейчас доступны 1K, 2K и 4K; 0.5K отклоняется локально до live-проверки этой private capability. Самый дешёвый размер, на который реально можно планировать бюджет сегодня, — 1K по $0.0336 за image output.",
        "当前订阅路由不支持。现有可用尺寸为 1K、2K、4K；在该私有能力完成实时验证前，0.5K 会被本地拒绝。目前真正能纳入预算的最低尺寸是 1K，每张图像输出 $0.0336。",
        "현재 subscription route에서는 불가합니다. 1K, 2K, 4K만 허용되며 0.5K는 해당 private capability가 live 검증되기 전까지 로컬에서 거부됩니다. 오늘 실제로 예산을 잡을 수 있는 가장 저렴한 크기는 1K로, image output당 $0.0336입니다.",
      ),
    ),
    faq(
      tr("Does prompt caching make repeated Nano Banana 2 calls cheaper?", "Делает ли кэширование промптов повторные вызовы Nano Banana 2 дешевле?", "提示词缓存会让重复的 Nano Banana 2 调用更便宜吗？", "프롬프트 캐싱으로 반복적인 Nano Banana 2 호출이 저렴해지나요?"),
      tr(
        "No. Unlike the text-oriented tiers, this image model bills cached input at the full input rate, so re-sending the same references costs the same every time. The cost controls that actually work are the image size, a bounded reference set and a per-key lifetime spending limit.",
        "Нет. В отличие от текстовых тарифов, эта image-модель оплачивает cached input по полной input-ставке, поэтому повторная отправка тех же references каждый раз стоит столько же. Реально работающие рычаги — размер изображения, ограниченный набор references и общий лимит расходов на ключ.",
        "不会。与文本档位不同，该图像模型的 cached input 按完整 input 费率计费，重复发送同样的参考图每次花费相同。真正有效的成本控制手段是图像尺寸、有界的参考图集合，以及每个密钥的终身消费上限。",
        "아닙니다. 텍스트 중심 등급과 달리 이 image 모델은 cached input도 전체 input 요율로 청구하므로 같은 reference를 다시 보낼 때마다 비용이 동일합니다. 실제로 효과 있는 비용 통제 수단은 이미지 크기, 제한된 reference 세트, key별 평생 누적 지출 한도입니다.",
      ),
    ),
    faq(
      tr("How do I pay for Nano Banana 2 usage, and can I test it free?", "Как оплатить использование Nano Banana 2 и можно ли протестировать бесплатно?", "如何支付 Nano Banana 2 的使用费用？可以免费测试吗？", "Nano Banana 2 사용료는 어떻게 결제하고, 물로 체험할 수 있나요?"),
      tr(
        "Usage draws from a prepaid balance topped up by bank card or crypto (USDT, BTC and other major coins) in any whole-dollar amount, and the balance never expires. Accounts created with Google or GitHub start with $5 of platform bonus credit — valid on supported Claude, GPT, Gemini and Kimi models and spent before paid balance; email/password accounts do not receive the bonus.",
        "Использование списывается с предоплаченного баланса, который пополняется банковской картой или криптой (USDT, BTC и другие основные монеты) на любую сумму в целых долларах; баланс не сгорает. Аккаунты через Google или GitHub начинают с бонуса $5 на баланс платформы — он действует на поддерживаемые модели Claude, GPT, Gemini и Kimi и тратится раньше оплаченного баланса; аккаунты через email/password бонус не получают.",
        "费用从预付余额中扣除，可通过银行卡或加密货币（USDT、BTC 及其他主流币种）充值任意整数美元，余额永不过期。通过 Google 或 GitHub 注册的账户可获得 $5 平台赠送额度，适用于支持的 Claude、GPT、Gemini 和 Kimi 模型，并先于付费余额消耗；邮箱/密码注册的账户不享受该额度。",
        "사용량은 선불 잔액에서 차감되며, 은행 카드 또는 암호화폐(USDT, BTC 및 기타 주요 코인)로 정수 달러 단위로 충전하고 잔액은 만료되지 않습니다. Google 또는 GitHub로 만든 계정은 $5 플랫폼 보너스 크레딧으로 시작하며 지원되는 Claude, GPT, Gemini, Kimi 모델에 사용할 수 있고 유료 잔액보다 먼저 소진됩니다. 이메일/비밀번호 계정은 보너스를 받지 않습니다.",
      ),
    ),
  ],
};
