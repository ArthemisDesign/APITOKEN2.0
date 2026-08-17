import type { ImageSeoSpec } from "./shared";
import { faq, list, note, paragraph, section, sharedCode, steps, table, tr, ROUTER, OPENAI } from "./shared";

export const spec: ImageSeoSpec = {
    slug: "nano-banana-2-vs-gpt-image-2",
    cluster: "compare",
    related: ["nano-banana-2-api-cost", "gpt-image-2-api-cost", "cheapest-image-generation-api", "image-editing-api-guide"],
    title: tr(
      "Nano Banana 2 vs GPT Image 2 API: Price, Protocol and Output Contract",
      "Nano Banana 2 vs GPT Image 2 API: цена, protocol и контракт output",
      "Nano Banana 2 vs GPT Image 2 API：价格、协议与输出契约",
      "Nano Banana 2 vs GPT Image 2 API: 가격, protocol, output 계약 비교",
    ),
    h1: tr(
      "Nano Banana 2 vs GPT Image 2: which image API fits your workload",
      "Nano Banana 2 или GPT Image 2: какой image API подходит вашей задаче",
      "Nano Banana 2 与 GPT Image 2：哪个图像 API 适合你的工作负载",
      "Nano Banana 2 vs GPT Image 2: 내 workload에 맞는 image API 고르기",
    ),
    description: tr(
      "Nano Banana 2 vs GPT Image 2 on real request shapes, sizes, reference limits, published rates and the 50% B2C policy — with cost math per accepted asset, not per headline.",
      "Nano Banana 2 и GPT Image 2: реальные request shapes, размеры, лимиты references, опубликованные ставки и скидка 50% для B2C — с расчётом цены за принятый ассет, а не за headline.",
      "从真实请求结构、尺寸、参考图限制、已公布费率与 B2C 五折政策比较 Nano Banana 2 和 GPT Image 2，并按每个验收资产而非表面报价核算成本。",
      "실제 request shape, 크기, reference 제한, 공개 요금과 B2C 50% 정책으로 Nano Banana 2와 GPT Image 2를 비교하고 headline이 아닌 accepted asset당 비용을 계산합니다.",
    ),
    keywords: tr(
      ["nano banana 2 vs gpt image 2", "best image generation api", "gpt image vs gemini image", "nano banana api comparison", "ai image api comparison", "image generation api cost"],
      ["nano banana 2 или gpt image 2", "лучший api генерации изображений", "gpt image vs gemini image", "сравнение nano banana api", "сравнение ai image api", "стоимость image generation api"],
      ["nano banana 2 对比 gpt image 2", "最佳图像生成 api", "gpt image 对比 gemini image", "nano banana api 对比", "ai 图像 api 对比", "图像生成 api 成本"],
      ["nano banana 2 vs gpt image 2", "최고의 이미지 생성 api", "gpt image vs gemini image", "nano banana api 비교", "ai image api 비교", "이미지 생성 api 비용"],
    ),
    dek: tr(
      "Nano Banana 2 gives explicit 1K/2K/4K sizes, broad aspect-ratio control and up to 14 references on the native Gemini shape. GPT Image 2 gives a narrow OpenAI Images route with one to five strict PNG references and terminal token billing. Both settle at 50% of exact official usage for regular B2C.",
      "Nano Banana 2 даёт явные размеры 1K/2K/4K, широкий контроль aspect ratio и до 14 references в native Gemini shape. GPT Image 2 — узкий OpenAI Images route с 1–5 строгими PNG и terminal token billing. Для обычного B2C обе модели стоят 50% от exact official usage.",
      "Nano Banana 2 在原生 Gemini 结构中提供明确的 1K/2K/4K 尺寸、丰富的宽高比控制与最多 14 张参考图。GPT Image 2 提供有界的 OpenAI Images 路由，支持 1–5 张严格 PNG，并按终态 token 计费。普通 B2C 两者均按准确官方用量五折结算。",
      "Nano Banana 2는 native Gemini 형식에서 명시적 1K/2K/4K 크기, 넓은 aspect-ratio 제어, 최대 14 references를 제공합니다. GPT Image 2는 1~5 strict PNG와 terminal token billing의 제한된 OpenAI Images route를 제공하며, 일반 B2C는 둘 다 exact official usage의 50%로 정산됩니다.",
    ),
    sections: [
      section(
        tr("Two contracts, not two interchangeable endpoints", "Два контракта, а не взаимозаменяемые endpoints", "两套契约，而非可互换的端点", "교환 불가한 endpoint가 아닌 두 가지 계약"),
        [
          paragraph(
            "Choose Nano Banana 2 when your pipeline needs explicit 1K/2K/4K sizing, wide aspect-ratio control or more than five reference images; choose GPT Image 2 when your stack already speaks the OpenAI Images API and one non-streaming base64 PNG of auto size is an acceptable output. On apiToken.sale both settle at 50% of exact official usage for a regular B2C account, so the real decision is contract fit and acceptance rate, not a headline price.",
            "Выбирайте Nano Banana 2, когда конвейеру нужны явные размеры 1K/2K/4K, широкий контроль aspect ratio или больше пяти references; GPT Image 2 — когда стек уже говорит на OpenAI Images API и приемлем один non-streaming base64 PNG размера auto. На apiToken.sale обе модели для обычного B2C стоят 50% от exact official usage, поэтому решают контракт и acceptance rate, а не цена в заголовке.",
            "当流水线需要明确的 1K/2K/4K 尺寸、灵活的宽高比控制或超过五张参考图时选 Nano Banana 2；当现有技术栈已使用 OpenAI Images API、且可接受单张非流式 auto 尺寸 base64 PNG 时选 GPT Image 2。在 apiToken.sale 上，普通 B2C 两者均按准确官方用量五折结算，因此真正决定因素契约匹配度与验收率，而非表面价格。",
            "파이프라인에 명시적 1K/2K/4K 크기, 넓은 aspect-ratio 제어 또는 5장 초과 references가 필요하면 Nano Banana 2를, 스택이 이미 OpenAI Images API를 쓰고 auto 크기의 non-streaming base64 PNG 한 장으로 충분하면 GPT Image 2를 선택하세요. apiToken.sale에서 일반 B2C는 둘 다 exact official usage의 50%로 정산되므로 결정 기준은 headline 가격이 아니라 계약 적합성과 acceptance rate입니다.",
          ),
          table(
            { headers: ["Decision", "Nano Banana 2", "GPT Image 2"], rows: [["Model ID", "gemini-3.1-flash-image", "gpt-image-2"], ["Endpoint", "POST /v1beta/models/gemini-3.1-flash-image:generateContent", "POST /v1/images/generations and /v1/images/edits"], ["Auth header", "x-goog-api-key", "Authorization: Bearer"], ["Output", "inlineData image part (base64 + MIME) in the candidates content", "one non-streaming base64 PNG in data[0].b64_json"], ["Published sizes", "1K, 2K, 4K (0.5K rejected on the live route)", "auto; exact dimensions not promised"], ["References", "up to 14 image inputs: PNG, JPEG, WEBP, HEIC, HEIF", "1–5 strict PNG files on the edits route"], ["Controls", "imageSize + aspectRatio in generationConfig.imageConfig", "background opaque, quality low, size auto"], ["Billing authority", "fixed image tokens by size plus input, text/thinking and grounding legs", "terminal text/image input, cached input and image-output usage"]] },
            { headers: ["Критерий", "Nano Banana 2", "GPT Image 2"], rows: [["Model ID", "gemini-3.1-flash-image", "gpt-image-2"], ["Endpoint", "POST /v1beta/models/gemini-3.1-flash-image:generateContent", "POST /v1/images/generations и /v1/images/edits"], ["Auth header", "x-goog-api-key", "Authorization: Bearer"], ["Output", "inlineData image part (base64 + MIME) внутри candidates content", "один non-streaming base64 PNG в data[0].b64_json"], ["Размеры", "1K, 2K, 4K (0.5K отклоняется на live route)", "auto; точные dimensions не обещаны"], ["References", "до 14 image inputs: PNG, JPEG, WEBP, HEIC, HEIF", "1–5 строгих PNG на edits route"], ["Controls", "imageSize + aspectRatio в generationConfig.imageConfig", "background opaque, quality low, size auto"], ["Billing authority", "фиксированные image tokens по размеру плюс input, text/thinking и grounding legs", "terminal text/image input, cached input и image-output usage"]] },
            { headers: ["决策项", "Nano Banana 2", "GPT Image 2"], rows: [["模型 ID", "gemini-3.1-flash-image", "gpt-image-2"], ["端点", "POST /v1beta/models/gemini-3.1-flash-image:generateContent", "POST /v1/images/generations 与 /v1/images/edits"], ["认证头", "x-goog-api-key", "Authorization: Bearer"], ["输出", "candidates 内容中的 inlineData 图像 part（base64 + MIME）", "data[0].b64_json 中的单张非流式 base64 PNG"], ["已发布尺寸", "1K、2K、4K（live 路由拒绝 0.5K）", "auto；不承诺准确尺寸"], ["参考图", "最多 14 张图像输入：PNG、JPEG、WEBP、HEIC、HEIF", "编辑路由上 1–5 张严格 PNG"], ["控制", "generationConfig.imageConfig 中的 imageSize + aspectRatio", "background opaque、quality low、size auto"], ["结算权威", "按尺寸固定 image token，外加 input、text/thinking 与 grounding 项", "终态 text/image input、cached input 与 image-output usage"]] },
            { headers: ["결정 기준", "Nano Banana 2", "GPT Image 2"], rows: [["모델 ID", "gemini-3.1-flash-image", "gpt-image-2"], ["Endpoint", "POST /v1beta/models/gemini-3.1-flash-image:generateContent", "POST /v1/images/generations 및 /v1/images/edits"], ["Auth header", "x-goog-api-key", "Authorization: Bearer"], ["Output", "candidates content 안의 inlineData image part(base64 + MIME)", "data[0].b64_json의 non-streaming base64 PNG 한 장"], ["공개 크기", "1K, 2K, 4K(live route에서 0.5K 거부)", "auto; 정확한 dimensions 미보장"], ["References", "최대 14개 image input: PNG, JPEG, WEBP, HEIC, HEIF", "edits route에서 strict PNG 1~5개"], ["Controls", "generationConfig.imageConfig의 imageSize + aspectRatio", "background opaque, quality low, size auto"], ["Billing authority", "크기별 고정 image token + input, text/thinking, grounding leg", "terminal text/image input, cached input, image-output usage"]] },
          ),
          note(
            "The protocols are not interchangeable. A Gemini inlineData response breaks a client that only parses the OpenAI Images schema, and the reverse is just as fatal — pin one parser per model instead of writing a universal one.",
            "Protocols невзаимозаменяемы: Gemini inlineData сломает client, понимающий только OpenAI Images schema, и наоборот. Закрепите по parser на модель вместо универсального.",
            "两种协议不可互换：Gemini inlineData 响应会让只解析 OpenAI Images schema 的客户端失败，反之亦然。应为每个模型固定各自的解析器，而不是写一个通用解析器。",
            "protocol은 교환할 수 없습니다. Gemini inlineData는 OpenAI Images schema만 아는 client를 깨고 반대도 마찬가지이므로 범용 parser 대신 모델별 parser를 고정하세요.",
          ),
        ],
      ),
      section(
        tr("Request and response shapes you actually integrate", "Реальные request/response shapes для интеграции", "实际接入的请求与响应结构", "실제로 통합하는 request/response 형태"),
        [
          sharedCode(`curl ${ROUTER}/v1beta/models/gemini-3.1-flash-image:generateContent \\
  -H "x-goog-api-key: $APITOKEN_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{"contents":[{"parts":[{"text":"Studio product photo on a light grey background"}]}],"generationConfig":{"responseModalities":["TEXT","IMAGE"],"imageConfig":{"imageSize":"1K","aspectRatio":"1:1"}}}'`),
          sharedCode(`curl ${OPENAI}/images/generations \\
  -H "Authorization: Bearer $APITOKEN_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{"model":"gpt-image-2","prompt":"Studio product photo on a light grey background","background":"opaque","quality":"low","size":"auto"}'`),
          paragraph(
            "The Gemini route answers with candidates[0].content.parts: text parts plus an inlineData part whose data field carries the base64 image and whose mimeType gives the format — decode the part, never scrape markdown. The OpenAI Images route returns one JSON document with the complete PNG in data[0].b64_json; there is no streaming variant on this subscription route, so delivery is all-or-nothing and the client should treat the response as a single atomic artifact.",
            "Gemini route отвечает candidates[0].content.parts: текстовые parts плюс inlineData part, где data содержит base64-изображение, а mimeType — формат; декодируйте part, а не парсьте markdown. OpenAI Images route возвращает один JSON-документ с полным PNG в data[0].b64_json; streaming-варианта на этом subscription route нет, поэтому доставка атомарна — всё или ничего.",
            "Gemini 路由返回 candidates[0].content.parts：文本 part 加上 inlineData part，其 data 字段为 base64 图像，mimeType 标明格式——应解码该 part，而不是解析 markdown。OpenAI Images 路由返回单个 JSON 文档，完整 PNG 位于 data[0].b64_json；该订阅路由没有流式变体，交付是全有或全无，客户端应把响应当作单个原子产物。",
            "Gemini route는 candidates[0].content.parts로 응답합니다. text part와 함께 inlineData part의 data 필드에 base64 이미지가, mimeType에 형식이 있으므로 markdown이 아닌 part를 decode하세요. OpenAI Images route는 전체 PNG가 data[0].b64_json에 담긴 JSON 문서 하나를 반환하며 이 subscription route에는 streaming 변형이 없어 전달이 all-or-nothing입니다.",
          ),
          paragraph(
            "For edits, GPT Image 2 switches to a multipart POST /v1/images/edits with each strict PNG reference as an image field; Nano Banana 2 keeps the same generateContent call and simply adds one inlineData part per reference, up to fourteen. Both responses carry usage fields, but only the terminal usage event is the billing authority.",
            "Для edits GPT Image 2 переключается на multipart POST /v1/images/edits, где каждая строгая PNG reference передаётся полем image; Nano Banana 2 использует тот же generateContent и просто добавляет по inlineData part на reference — до четырнадцати. Оба ответа несут usage-поля, но billing authority — только terminal usage event.",
            "编辑时 GPT Image 2 切换为 multipart POST /v1/images/edits，每张严格 PNG 参考图作为 image 字段上传；Nano Banana 2 沿用同一个 generateContent 调用，为每张参考图添加一个 inlineData part，最多十四张。两种响应都带 usage 字段，但结算权威只有终态 usage event。",
            "edit에서 GPT Image 2는 각 strict PNG reference를 image 필드로 별도 첨부하는 multipart POST /v1/images/edits로 전환하고, Nano Banana 2는 같은 generateContent 호출에 reference당 inlineData part를 하나씩(최대 14개) 추가합니다. 두 응답 모두 usage 필드를 갖지만 billing authority는 terminal usage event뿐입니다.",
          ),
        ],
      ),
      section(
        tr("Cost math with the published rates", "Расчёт стоимости по опубликованным ставкам", "按已公布费率核算成本", "공개 요금으로 계산하는 비용"),
        [
          table(
            { headers: ["Cost component", "Nano Banana 2 (regular B2C)", "GPT Image 2 (regular B2C)"], rows: [["Text/prompt input", "$0.25/M (official $0.50/M)", "$2.50/M fresh, $0.625/M cached (official $5/M, $1.25/M)"], ["Reference image input", "input tokens at the model rate; no cache discount", "$4/M fresh, $1/M cached (official $8/M, $2/M)"], ["Image output", "fixed by size: 1K $0.0336, 2K $0.0504, 4K $0.0756", "$15/M of actual image-output tokens (official $30/M)"], ["Cache rule", "cached input billed at the full input rate", "cached input at 25% of fresh before the 50% discount"]] },
            { headers: ["Составляющая", "Nano Banana 2 (обычный B2C)", "GPT Image 2 (обычный B2C)"], rows: [["Text/prompt input", "$0.25/M (официально $0.50/M)", "$2.50/M fresh, $0.625/M cached (официально $5/M, $1.25/M)"], ["Reference image input", "input tokens по ставке модели; скидки на cache нет", "$4/M fresh, $1/M cached (официально $8/M, $2/M)"], ["Image output", "фиксировано по размеру: 1K $0.0336, 2K $0.0504, 4K $0.0756", "$15/M фактических image-output tokens (официально $30/M)"], ["Cache rule", "cached input по полной input-ставке", "cached input = 25% от fresh до применения скидки 50%"]] },
            { headers: ["成本项", "Nano Banana 2（普通 B2C）", "GPT Image 2（普通 B2C）"], rows: [["文本/提示词输入", "$0.25/M（官方 $0.50/M）", "fresh $2.50/M，cached $0.625/M（官方 $5/M、$1.25/M）"], ["参考图输入", "按模型费率计 input token；无缓存折扣", "fresh $4/M，cached $1/M（官方 $8/M、$2/M）"], ["图像输出", "按尺寸固定：1K $0.0336、2K $0.0504、4K $0.0756", "实际 image-output token $15/M（官方 $30/M）"], ["缓存规则", "cached input 按完整 input 费率计费", "cached input 先按 fresh 的 25% 计，再享五折"]] },
            { headers: ["비용 항목", "Nano Banana 2 (일반 B2C)", "GPT Image 2 (일반 B2C)"], rows: [["텍스트/prompt input", "$0.25/M (공식 $0.50/M)", "fresh $2.50/M, cached $0.625/M (공식 $5/M, $1.25/M)"], ["Reference image input", "모델 요금의 input token; cache 할인 없음", "fresh $4/M, cached $1/M (공식 $8/M, $2/M)"], ["Image output", "크기별 고정: 1K $0.0336, 2K $0.0504, 4K $0.0756", "실제 image-output token $15/M (공식 $30/M)"], ["Cache rule", "cached input은 전체 input 요금", "cached input은 fresh의 25% 계산 후 50% 할인"]] },
          ),
          paragraph(
            "A 1K Nano Banana 2 render costs a fixed $0.0336 of image output plus the prompt: a 400-token brief adds 400 × $0.25/1M = $0.0001, so the request settles near $0.0337 before any optional grounding. GPT Image 2 has no fixed sticker: if terminal usage reports 1,600 image-output tokens and 600 fresh text-input tokens, the same brief costs 1,600 × $15/1M + 600 × $2.50/1M = $0.0255 — lower on that sample, but the number moves with every request because neither dimensions nor output tokens are contracted. Budget Nano Banana 2 from the size table; budget GPT Image 2 from measured terminal usage on your own prompts.",
            "Рендер 1K Nano Banana 2 стоит фиксированные $0.0336 за image output плюс prompt: brief на 400 tokens добавляет 400 × $0.25/1M = $0.0001, итого около $0.0337 до возможного grounding. У GPT Image 2 фиксированной цены нет: если terminal usage показывает 1 600 image-output tokens и 600 fresh text-input tokens, тот же brief стоит 1 600 × $15/1M + 600 × $2.50/1M = $0.0255 — на этом примере меньше, но число меняется с каждым запросом, потому что ни dimensions, ни output tokens не зафиксированы контрактом. Бюджет Nano Banana 2 считайте по таблице размеров, GPT Image 2 — по измеренному terminal usage на своих prompts.",
            "1K Nano Banana 2 渲染的图像输出固定为 $0.0336，再加提示词：400 token 的 brief 增加 400 × $0.25/1M = $0.0001，不计可选 grounding 时整单约 $0.0337。GPT Image 2 没有固定牌价：若终态 usage 报告 1,600 个 image-output token 和 600 个 fresh text-input token，同一 brief 成本为 1,600 × $15/1M + 600 × $2.50/1M = $0.0255——该样本更低，但由于尺寸与输出 token 均无契约保证，每次请求的数字都会变。Nano Banana 2 按尺寸表做预算，GPT Image 2 按自己提示词实测的终态 usage 做预算。",
            "1K Nano Banana 2 렌더는 고정 $0.0336의 image output에 prompt가 더해집니다. 400 token brief는 400 × $0.25/1M = $0.0001을 더해 선택적 grounding 전 약 $0.0337에 정산됩니다. GPT Image 2는 고정 가격이 없어 terminal usage가 image-output 1,600 token, fresh text-input 600 token을 보고하면 같은 brief는 1,600 × $15/1M + 600 × $2.50/1M = $0.0255입니다. 이 샘플에서는 더 낮지만 dimensions와 output token 모두 계약되지 않아 요청마다 숫자가 바뀝니다. Nano Banana 2는 크기 표로, GPT Image 2는 실제 prompt의 측정된 terminal usage로 예산을 잡으세요.",
          ),
          note(
            "Compare cost per accepted asset, not per request. A model that needs two retries to pass your visual checklist costs double its nominal rate — acceptance rate is the multiplier the price table never shows.",
            "Сравнивайте цену за принятый ассет, а не за запрос: модель, которой нужны два retries для прохождения visual checklist, стоит вдвое дороже номинала — acceptance rate это множитель, которого нет в таблице цен.",
            "应比较每个验收资产的成本而非每次请求：需要两次重试才能通过视觉清单的模型，实际成本是名义费率的两倍——验收率是价格表永远看不到的乘数。",
            "요청당이 아닌 accepted asset당 비용을 비교하세요. visual checklist 통과에 retry 두 번이 필요한 모델은 명목 요금의 두 배입니다. acceptance rate는 가격표에 없는 승수입니다.",
          ),
        ],
      ),
      section(
        tr("Run a two-model bake-off before committing", "Проведите bake-off двух моделей до выбора", "定型前先做一次双模型对比评测", "확정 전 두 모델 bake-off 실행"),
        [
          steps(
            ["Build a fixed test set: text-only generation, one-reference editing and your hardest aspect ratio, drawn from real catalog or campaign briefs.", "Run the identical brief on both models without silently changing resolution, quality or the reference set.", "Score instruction fidelity, product/reference fidelity, artifacts and latency, then attach the settled charge from terminal usage to every output.", "Compute cost per accepted asset, including the failed outputs, storage and human review each candidate required.", "Pin the winner per asset class — catalog photos, lifestyle scenes, text-heavy graphics — instead of crowning one global default without evidence."],
            ["Соберите фиксированный test set: text-only generation, edit с одной reference и самый сложный aspect ratio — на реальных catalog/campaign briefs.", "Запустите одинаковый brief на обеих моделях без скрытой смены resolution, quality или набора references.", "Оцените instruction fidelity, product/reference fidelity, artifacts и latency, затем привяжите settled charge из terminal usage к каждому output.", "Посчитайте цену за принятый ассет с учётом неудачных outputs, storage и human review каждого кандидата.", "Закрепите победителя по классу ассетов — catalog photos, lifestyle scenes, text-heavy graphics — а не назначайте один global default без evidence."],
            ["建立固定测试集：纯文本生成、单参考图编辑与最难的宽高比，素材取自真实目录或营销 brief。", "两种模型运行完全相同的 brief，不要静默改变分辨率、质量或参考图集合。", "评分指令遵循度、产品/参考图一致性、瑕疵与延迟，并把终态 usage 的结算费用附加到每个输出。", "计算每个验收资产的成本，计入每个候选的失败输出、存储与人工审核。", "按资产类别（目录照片、生活方式场景、文字密集图形）固定胜出模型，而不是在没有证据时设一个全局默认。"],
            ["실제 catalog/campaign brief에서 text-only generation, reference 한 장 edit, 가장 어려운 aspect ratio로 고정 test set을 만듭니다.", "resolution, quality, reference 세트를 몰래 바꾸지 않고 두 모델에 동일한 brief를 실행합니다.", "instruction fidelity, product/reference fidelity, artifact, latency를 채점하고 terminal usage의 settled charge를 각 output에 연결합니다.", "실패 output, storage, human review를 포함해 후볳별 accepted asset당 비용을 계산합니다.", "catalog photo, lifestyle scene, text-heavy graphic 등 asset class별로 승자를 고정하고 근거 없는 global default를 두지 않습니다."],
          ),
          paragraph(
            "The pattern that emerges in practice: Nano Banana 2 wins asset classes where explicit size or aspect control and many references are contractual — ecommerce batch pipelines that must ship exact 1K or 4K masters, or editing flows that blend one product shot with several scene references. GPT Image 2 wins where an existing OpenAI Images client, a strict-PNG edit pipeline or a one-output contract cuts integration risk. Neither verdict removes the need for your own eval.",
            "На практике картина такая: Nano Banana 2 выигрывает классы ассетов, где явные size/aspect controls и много references — требование контракта: ecommerce batch-пайплайны, обязанные отгружать точные 1K или 4K мастера, и editing flows, смешивающие product shot с несколькими scene references. GPT Image 2 выигрывает там, где существующий OpenAI Images client, strict-PNG edit pipeline или контракт одного output снижают integration risk. Ни один verdict не отменяет собственный eval.",
            "实践中呈现的规律是：当明确尺寸/宽高比控制与多参考图属于契约要求时——例如必须交付精确 1K 或 4K 母版的电商批量流水线、或把一张产品图与多张场景参考图融合的编辑流程——Nano Banana 2 胜出；当已有 OpenAI Images 客户端、严格 PNG 编辑流程或单输出契约能降低集成风险时，GPT Image 2 胜出。任何结论都不能替代自己的评测。",
            "실무에서 드러나는 패턴은 이렇습니다. 명시적 size/aspect control과 다수 references가 계약 요건인 asset class—정확한 1K/4K master를 배송해야 하는 ecommerce batch pipeline이나 product shot 하나와 여러 scene reference를 섞는 editing flow—에서는 Nano Banana 2가, 기존 OpenAI Images client·strict-PNG edit pipeline·one-output 계약이 integration risk를 줄이는 곳에서는 GPT Image 2가 이깁니다. 어떤 판정도 자체 eval을 대신하지 못합니다.",
          ),
        ],
      ),
      section(
        tr("One key and one prepaid balance for both models", "Один ключ и один prepaid-баланс для обеих моделей", "一个密钥、一个预付余额调用两款模型", "두 모델에 하나의 key와 prepaid 잔액"),
        [
          list(
            ["Both endpoints sit behind the same apiToken.sale key at router.apitoken.sale, so a two-model evaluation needs no second account or provider signup.", "Regular B2C accounts pay 50% of exact official usage on both models; B2B follows negotiated terms and OpenKeys bill 1:1 at official prices.", "Accounts created with Google or GitHub receive a $5 welcome bonus, and free credit is always spent before paid balance — the whole bake-off above can run on the grant.", "Protect the image worker with its own key carrying a lifetime spending limit and an expiration date, so a batch loop cannot drain the account.", "Top up by bank card or crypto in any whole USD amount; the balance is consumed only when requests run."],
            ["Оба endpoint работают за одним ключом apiToken.sale на router.apitoken.sale — для оценки двух моделей не нужны второй аккаунт и регистрация у provider.", "Обычный B2C платит 50% от exact official usage за обе модели; у B2B — согласованные условия, OpenKeys тарифицируются 1:1 по официальной цене.", "Аккаунты, созданные через Google или GitHub, получают welcome bonus $5, а free credit всегда расходуется раньше paid balance — весь bake-off выше можно провести на гранте.", "Защитите image worker отдельным ключом с общим лимитом расходов и датой истечения, чтобы batch-цикл не истощил аккаунт.", "Пополняйте баланс банковской картой или криптой на любую целую сумму в USD; средства списываются только за выполненные запросы."],
            ["两个端点共用 router.apitoken.sale 上的同一把 apiToken.sale 密钥，双模型评测无需第二个账户或提供商注册。", "普通 B2C 账户对两款模型均按准确官方用量的 50% 付费；B2B 按协商条款，OpenKeys 按官方价格 1:1 计费。", "通过 Google 或 GitHub 创建的账户可获得 $5 欢迎奖励，且免费额度总是先于付费余额消耗——上面的整个对比评测都可以用这笔赠金完成。", "为图像 worker 配置独立密钥，设置终身消费上限与到期日期，防止批量循环耗尽账户余额。", "可用银行卡或加密货币按任意整数美元金额充值，余额仅在请求执行时消耗。"],
            ["두 endpoint 모두 router.apitoken.sale의 같은 apiToken.sale key 뒤에 있어 두 모델 평가에 두 번째 계정이나 provider 가입이 필요 없습니다.", "일반 B2C 계정은 두 모델 모두 exact official usage의 50%를 납부하고 B2B는 협상 조건, OpenKeys는 공식 가격 1:1입니다.", "Google 또는 GitHub로 만든 계정은 $5 welcome bonus를 받고 free credit은 항상 paid balance보다 먼저 소진되므로 위 bake-off 전체를 grant로 실행할 수 있습니다.", "image worker에는 평생 누적 지출 한도와 만료일이 있는 전용 key를 발급해 batch loop가 계정을 소진하지 못하게 합니다.", "은행 카드나 crypto로 임의의 정수 USD 금액을 충전하며 잔액은 요청이 실행될 때만 소모됩니다."],
          ),
          paragraph(
            "Whichever model wins an asset class, store the request ID and the terminal usage event next to the generated asset. The dashboard charge should match that usage exactly; a mismatch is grounds for reconciliation, not a rounding error.",
            "Какая бы модель ни выиграла класс ассетов, храните request ID и terminal usage event рядом со сгенерированным ассетом. Charge в дашборде должен точно совпадать с usage; расхождение — повод для reconciliation, а не ошибка округления.",
            "无论哪款模型赢得某个资产类别，都应把 request ID 与终态 usage event 和生成的资产一起保存。仪表板扣费应与该 usage 精确一致；出现差异就应核对，而不是当作舍入误差。",
            "어느 모델이 asset class를 가져가든 request ID와 terminal usage event를 생성된 asset 옆에 저장하세요. dashboard charge는 usage와 정확히 일치해야 하며 불일치는 반올림 오류가 아니라 대조가 필요한 신호입니다.",
          ),
        ],
      ),
    ],
    faq: [
      faq(
        tr("Is Nano Banana 2 or GPT Image 2 cheaper per image?", "Что дешевле за изображение: Nano Banana 2 или GPT Image 2?", "Nano Banana 2 和 GPT Image 2 哪款的单张图像更便宜？", "Nano Banana 2와 GPT Image 2 중 이미지당 어느 쪽이 더 저렴한가요?"),
        tr("There is no universal winner. Nano Banana 2 has fixed image-output legs of $0.0336 / $0.0504 / $0.0756 for 1K / 2K / 4K on regular B2C, plus bounded input; GPT Image 2 settles actual tokens at $15/M image output for regular B2C, so its per-image price moves with every request. Compare cost per accepted asset on your own prompts and references.", "Универсального победителя нет. У Nano Banana 2 фиксированные image-output составляющие $0.0336 / $0.0504 / $0.0756 за 1K / 2K / 4K для обычного B2C плюс ограниченный input; GPT Image 2 рассчитывает фактические tokens по $15/M за image output для обычного B2C, и цена картинки меняется с каждым запросом. Сравнивайте цену за принятый ассет на своих prompts и references.", "没有通用答案。普通 B2C 下 Nano Banana 2 的图像输出固定为 1K $0.0336、2K $0.0504、4K $0.0756，外加有界输入；GPT Image 2 按实际 token 结算，普通 B2C 图像输出 $15/M，单张价格随每次请求变化。应使用自己的提示词与参考图比较每个验收资产的成本。", "보편적 승자는 없습니다. 일반 B2C에서 Nano Banana 2는 1K/2K/4K 고정 image-output leg가 $0.0336/$0.0504/$0.0756에 제한된 input이 더해지고, GPT Image 2는 image output $15/M로 실제 token을 정산해 이미지당 가격이 요청마다 변합니다. 실제 prompt와 reference로 accepted asset당 비용을 비교하세요."),
      ),
      faq(
        tr("How many reference images can each model use?", "Сколько references поддерживает каждая модель?", "每款模型最多支持多少张参考图？", "각 모델은 reference image를 몇 장까지 지원하나요?"),
        tr("Nano Banana 2 accepts up to 14 supported image inputs in PNG, JPEG, WEBP, HEIC or HEIF. GPT Image 2 accepts one to five strict PNG files on its multipart edits route; convert other formats to PNG and validate them before upload.", "Nano Banana 2 принимает до 14 поддерживаемых image inputs в форматах PNG, JPEG, WEBP, HEIC, HEIF. GPT Image 2 принимает 1–5 строгих PNG на multipart edits route; остальные форматы конвертируйте в PNG и проверяйте перед upload.", "Nano Banana 2 最多接受 14 张受支持图像输入，格式为 PNG、JPEG、WEBP、HEIC、HEIF。GPT Image 2 在 multipart 编辑路由上接受 1–5 张严格 PNG；其他格式需先转换并验证为 PNG 再上传。", "Nano Banana 2는 PNG, JPEG, WEBP, HEIC, HEIF의 지원 image input을 최대 14개 받습니다. GPT Image 2는 multipart edits route에서 strict PNG 1~5개만 받으므로 다른 형식은 PNG로 변환·검증 후 업로드하세요."),
      ),
      faq(
        tr("Can I call both models with the same request format?", "Можно ли вызвать обе модели одним request format?", "两款模型能用同一种请求格式调用吗？", "두 모델을 같은 request format으로 호출할 수 있나요?"),
        tr("No. Nano Banana 2 uses the native Gemini generateContent shape with the x-goog-api-key header; GPT Image 2 uses OpenAI Images routes with Authorization: Bearer. One apiToken.sale key works for both, but the request body, response parser and output contract differ per model.", "Нет. Nano Banana 2 использует native Gemini generateContent с заголовком x-goog-api-key; GPT Image 2 — OpenAI Images routes с Authorization: Bearer. Один ключ apiToken.sale подходит обеим, но request body, response parser и контракт output у моделей разные.", "不能。Nano Banana 2 使用原生 Gemini generateContent 结构与 x-goog-api-key 请求头；GPT Image 2 使用 OpenAI Images 路由与 Authorization: Bearer。同一把 apiToken.sale 密钥两者通用，但请求体、响应解析器与输出契约因模型而异。", "아닙니다. Nano Banana 2는 x-goog-api-key 헤더의 native Gemini generateContent 형식을, GPT Image 2는 Authorization: Bearer의 OpenAI Images route를 사용합니다. 하나의 apiToken.sale key로 둘 다 호출되지만 request body, response parser, output 계약은 모델마다 다릅니다."),
      ),
      faq(
        tr("Does the 50% discount apply to both models?", "Скидка 50% действует на обе модели?", "两款模型都享受五折吗？", "두 모델 모두 50% 할인이 적용되나요?"),
        tr("Yes, for regular B2C accounts: after exact official usage is calculated, both models settle at half price. B2B accounts follow their negotiated policy and OpenKeys bill 1:1 at official prices.", "Да, для обычных B2C-аккаунтов: после расчёта exact official usage обе модели оплачиваются вполовину. У B2B действует согласованная политика, а OpenKeys тарифицируются 1:1 по официальной цене.", "普通 B2C 账户可以：在准确官方用量计算完成后，两款模型均按半价结算。B2B 账户按协商策略，OpenKeys 按官方价格 1:1 计费。", "일반 B2C 계정에는 적용됩니다. exact official usage 계산 후 두 모델 모두 반값에 정산됩니다. B2B 계정은 협상 정책을, OpenKeys는 공식 가격 1:1을 따릅니다."),
      ),
      faq(
        tr("Which model guarantees exact output dimensions?", "Какая модель гарантирует точные dimensions output?", "哪款模型能保证准确的输出尺寸？", "어느 모델이 정확한 output dimensions를 보장하나요?"),
        tr("Only Nano Banana 2 publishes explicit sizes: 1K, 2K and 4K, selected per request through imageConfig.imageSize. GPT Image 2 takes size:auto and exact dimensions are not promised on this subscription wire — if a contract demands pixel-exact masters, validate the delivered dimensions either way.", "Только Nano Banana 2 публикует явные размеры: 1K, 2K и 4K, выбираемые через imageConfig.imageSize. GPT Image 2 принимает size:auto, и точные dimensions на этом subscription wire не обещаны — если контракт требует pixel-exact мастера, в любом случае проверяйте доставленные dimensions.", "只有 Nano Banana 2 公布明确尺寸：1K、2K、4K，可通过 imageConfig.imageSize 按请求选择。GPT Image 2 使用 size:auto，该订阅传输不承诺准确尺寸——若契约要求像素级精确母版，无论用哪款都应校验交付尺寸。", "명시적 크기를 공개하는 것은 Nano Banana 2뿐입니다. imageConfig.imageSize로 요청별 1K, 2K, 4K를 선택합니다. GPT Image 2는 size:auto를 받고 이 subscription wire에서 정확한 dimensions를 보장하지 않으므로 pixel-exact master가 필요하면 어느 쪽이든 전달된 dimensions를 검증하세요."),
      ),
      faq(
        tr("Can I evaluate both models before paying?", "Можно ли оценить обе модели до оплаты?", "能在付费前评测两款模型吗？", "결제 전에 두 모델을 평가할 수 있나요?"),
        tr("Yes. Register with Google or GitHub and the $5 welcome bonus lands on the same prepaid balance; free credit is always spent before paid balance, and both models run against the same production endpoints, so the evaluation measures exactly what you would buy.", "Да. Зарегистрируйтесь через Google или GitHub — welcome bonus $5 попадёт на тот же prepaid-баланс; free credit расходуется раньше paid balance, а обе модели работают на тех же production endpoints, так что оценка измеряет именно то, что вы купите.", "可以。通过 Google 或 GitHub 注册，$5 欢迎奖励会进入同一个预付余额；免费额度总是先于付费余额消耗，且两款模型运行在相同的生产端点上，因此评测衡量的正是你将要购买的服务。", "가능합니다. Google 또는 GitHub로 가입하면 $5 welcome bonus가 같은 prepaid 잔액에 적립되고 free credit은 항상 paid balance보다 먼저 소진됩니다. 두 모델 모두 같은 production endpoint에서 실행되므로 평가는 실제 구매할 서비스를 그대로 측정합니다."),
      ),
    ],
  };
