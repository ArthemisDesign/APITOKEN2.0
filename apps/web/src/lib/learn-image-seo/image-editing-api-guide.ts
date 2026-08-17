import type { ImageSeoSpec } from "./shared";
import { faq, list, note, paragraph, section, sharedCode, steps, table, tr, OPENAI, ROUTER } from "./shared";

export const spec: ImageSeoSpec = {
    slug: "image-editing-api-guide",
    cluster: "integrate",
    related: ["gpt-image-2-api-guide", "nano-banana-2-api-guide", "nano-banana-2-vs-gpt-image-2", "image-generation-api-for-ecommerce"],
    title: tr(
      "Image Editing API Guide: GPT Image 2 and Nano Banana 2",
      "API редактирования изображений: GPT Image 2 и Nano Banana 2",
      "图像编辑 API 指南：GPT Image 2 与 Nano Banana 2",
      "이미지 편집 API 가이드: GPT Image 2와 Nano Banana 2",
    ),
    h1: tr(
      "Edit images through GPT Image 2 or Nano Banana 2",
      "Редактирование изображений через GPT Image 2 или Nano Banana 2",
      "使用 GPT Image 2 或 Nano Banana 2 编辑图像",
      "GPT Image 2 또는 Nano Banana 2로 이미지 편집",
    ),
    description: tr(
      "Build a production image-editing API workflow with GPT Image 2 or Nano Banana 2: reference contracts, request and response shapes, worked cost math, validation gates and safe retry rules.",
      "Постройте production image-editing workflow на GPT Image 2 или Nano Banana 2: reference contracts, форматы request/response, расчёт стоимости, validation gates и безопасные retry rules.",
      "使用 GPT Image 2 或 Nano Banana 2 构建生产级图像编辑 API 工作流：参考图契约、请求与响应结构、成本演算、验证关卡与安全重试规则。",
      "GPT Image 2 또는 Nano Banana 2로 production image-editing workflow를 구축하세요: reference contract, request/response 형식, 비용 계산, validation gate, 안전한 retry 규칙.",
    ),
    keywords: tr(
      ["image editing api", "ai image edit api", "gpt image 2 edit api", "nano banana image editing", "reference image api", "product image editing api"],
      ["api редактирования изображений", "ai image edit api", "gpt image 2 edit api", "nano banana редактирование", "reference image api", "api обработки фото товара"],
      ["图像编辑 api", "ai 图像编辑 api", "gpt image 2 编辑 api", "nano banana 图像编辑", "参考图 api", "产品图像编辑 api"],
      ["이미지 편집 api", "ai image edit api", "gpt image 2 edit api", "nano banana 이미지 편집", "reference image api", "상품 이미지 편집 api"],
    ),
    dek: tr(
      "GPT Image 2 exposes a strict multipart PNG edit route with one base64 output. Nano Banana 2 treats references as native multimodal input — more formats, more images, explicit sizes. Choose by the reference contract your pipeline can validate, and bill on terminal usage.",
      "GPT Image 2 предоставляет строгий multipart PNG edit route с одним base64 output. Nano Banana 2 принимает references как native multimodal input — больше форматов, больше изображений, явные размеры. Выбирайте по reference contract, который pipeline умеет валидировать, и считайте деньги по terminal usage.",
      "GPT Image 2 提供严格的 multipart PNG 编辑路由，输出单张 base64 图像；Nano Banana 2 把参考图作为原生多模态输入——格式更多、数量更多、尺寸明确。应按流水线可验证的参考图契约选型，并以终态 usage 结算。",
      "GPT Image 2는 strict multipart PNG edit route와 base64 output 한 장을 제공합니다. Nano Banana 2는 references를 native multimodal input으로 받아 더 많은 형식과 수량, 명시적 크기를 지원합니다. pipeline이 검증할 수 있는 reference contract로 선택하고 terminal usage로 정산하세요.",
    ),
    sections: [
      section(
        tr(
          "Pick the route your reference contract can satisfy",
          "Выберите route под ваш reference contract",
          "选择能满足参考图契约的路由",
          "reference contract에 맞는 route 선택",
        ),
        [
          paragraph(
            "An image edit is a generation request that carries reference images as billable input plus a prompt naming the one change you allow. A single prepaid apiToken.sale key reaches both production edit routes: GPT Image 2 on the OpenAI Images edits endpoint, which takes one to five strict PNG references and returns one non-streaming base64 PNG, and Nano Banana 2 — model gemini-3.1-flash-image — on the native Gemini generateContent route, which accepts up to 14 references in PNG, JPEG, WEBP, HEIC or HEIF and answers with an inlineData image part. Decide by the reference contract your application can validate, not by brand preference.",
            "Image edit — это generation request, который несёт reference images как billable input и prompt с одним разрешённым изменением. Один prepaid-ключ apiToken.sale открывает оба production edit routes: GPT Image 2 на OpenAI Images edits endpoint принимает 1–5 строгих PNG references и возвращает один non-streaming base64 PNG, а Nano Banana 2 (модель gemini-3.1-flash-image) на native Gemini generateContent route принимает до 14 references в PNG, JPEG, WEBP, HEIC или HEIF и отвечает image inlineData part. Выбирайте по reference contract, который приложение умеет валидировать, а не по бренду.",
            "图像编辑是一种携带参考图作为计费输入、并在提示词中只允许一项变更的生成请求。一个 apiToken.sale 预付密钥即可接入两条生产编辑路由：GPT Image 2 走 OpenAI Images 编辑端点，接受 1–5 张严格 PNG 参考图，返回单张非流式 base64 PNG；Nano Banana 2（模型 gemini-3.1-flash-image）走原生 Gemini generateContent 路由，最多接受 14 张 PNG、JPEG、WEBP、HEIC 或 HEIF 参考图，以 inlineData 图像 part 返回。应按应用可验证的参考图契约选型，而不是看品牌偏好。",
            "이미지 편집은 reference image를 billable input으로 싣고 prompt에 허용된 변경 하나만 명시하는 generation request입니다. apiToken.sale prepaid key 하나로 두 production edit route를 쓸 수 있습니다. OpenAI Images edits endpoint의 GPT Image 2는 strict PNG reference 1~5장을 받아 non-streaming base64 PNG 한 장을 반환하고, native Gemini generateContent route의 Nano Banana 2(모델 gemini-3.1-flash-image)는 PNG, JPEG, WEBP, HEIC, HEIF reference를 최대 14장 받아 inlineData image part로 응답합니다. 브랜드 선호가 아니라 애플리케이션이 검증할 수 있는 reference contract로 결정하세요.",
          ),
          table(
            { headers: ["Capability", "GPT Image 2", "Nano Banana 2"], rows: [["Route", "POST /v1/images/edits (multipart)", "generateContent with inlineData parts"], ["References", "1–5", "up to 14"], ["Input files", "strict PNG", "PNG, JPEG, WEBP, HEIC, HEIF"], ["Output", "one non-streaming base64 PNG", "image inlineData part"], ["Published controls", "background opaque, quality low, size auto", "1K/2K/4K + published aspect ratios"]] },
            { headers: ["Возможность", "GPT Image 2", "Nano Banana 2"], rows: [["Route", "POST /v1/images/edits (multipart)", "generateContent с inlineData parts"], ["References", "1–5", "до 14"], ["Input files", "строгий PNG", "PNG, JPEG, WEBP, HEIC, HEIF"], ["Output", "один non-streaming base64 PNG", "image inlineData part"], ["Published controls", "background opaque, quality low, size auto", "1K/2K/4K + published aspect ratios"]] },
            { headers: ["能力", "GPT Image 2", "Nano Banana 2"], rows: [["路由", "POST /v1/images/edits（multipart）", "含 inlineData part 的 generateContent"], ["参考图", "1–5", "最多 14"], ["输入文件", "严格 PNG", "PNG、JPEG、WEBP、HEIC、HEIF"], ["输出", "单张非流式 base64 PNG", "image inlineData part"], ["已发布控制", "background opaque、quality low、size auto", "1K/2K/4K + 已发布宽高比"]] },
            { headers: ["기능", "GPT Image 2", "Nano Banana 2"], rows: [["Route", "POST /v1/images/edits (multipart)", "inlineData part를 쓰는 generateContent"], ["References", "1~5", "최대 14"], ["Input files", "strict PNG", "PNG, JPEG, WEBP, HEIC, HEIF"], ["Output", "non-streaming base64 PNG 한 장", "image inlineData part"], ["Published controls", "background opaque, quality low, size auto", "1K/2K/4K + 공개 aspect ratio"]] },
          ),
          note(
            "The two protocols are not interchangeable. A client written for the OpenAI Images schema cannot parse a Gemini inlineData response, and the GPT Image 2 route rejects a JPEG reference outright. Commit to a route per asset class at design time; switching protocols inside a retry loop corrupts both output parsing and cost attribution.",
            "Эти protocols невзаимозаменяемы. Client, написанный под OpenAI Images schema, не разберёт Gemini inlineData response, а GPT Image 2 route сразу отклонит JPEG reference. Закрепите route за классом ассетов на design stage: смена protocol внутри retry loop ломает и parsing output, и cost attribution.",
            "两种协议不可互换。按 OpenAI Images schema 编写的客户端无法解析 Gemini inlineData 响应，GPT Image 2 路由也会直接拒绝 JPEG 参考图。应在设计阶段按资产类别固定路由；在重试循环中切换协议会破坏输出解析与成本归因。",
            "두 protocol은 교환할 수 없습니다. OpenAI Images schema용 client는 Gemini inlineData response를 파싱할 수 없고 GPT Image 2 route는 JPEG reference를 즉시 거부합니다. 설계 단계에서 asset class별로 route를 고정하세요. retry loop 안에서 protocol을 바꾸면 output 파싱과 cost attribution이 모두 깨집니다.",
          ),
        ],
      ),
      section(
        tr(
          "The GPT Image 2 edit: strict PNG in, one base64 PNG out",
          "Edit через GPT Image 2: strict PNG на входе, один base64 PNG на выходе",
          "GPT Image 2 编辑：严格 PNG 输入，单张 base64 PNG 输出",
          "GPT Image 2 edit: strict PNG 입력, base64 PNG 한 장 출력",
        ),
        [
          paragraph(
            "The edits endpoint is multipart form data: model, prompt and one image field per reference. The published profile on this route is deliberately narrow — background opaque, quality low, size auto — and the response is a single JSON document whose data array holds one base64-encoded PNG. Transparent backgrounds and exact pixel dimensions are not promised here; treat them as design-time rejections, not parameters to probe in production.",
            "Edits endpoint — это multipart form data: model, prompt и по одному image-полю на reference. Published profile этого route намеренно узкий — background opaque, quality low, size auto, — а response представляет собой один JSON-документ, чей data array содержит один base64-encoded PNG. Transparent background и точные pixel dimensions здесь не обещаны: считайте их отклонёнными на design stage, а не параметрами для проверки в production.",
            "编辑端点使用 multipart form data：model、prompt，以及每张参考图一个 image 字段。该路由的已发布配置刻意保持狭窄——background opaque、quality low、size auto——响应是单个 JSON 文档，其 data 数组包含一张 base64 编码的 PNG。透明背景与精确像素尺寸在此不作承诺：应在设计阶段就排除，而不是到生产环境再试探。",
            "edits endpoint는 multipart form data입니다. model, prompt, reference당 image 필드 하나씩을 본냅니다. 이 route의 공개 profile은 의도적으로 좁습니다. background opaque, quality low, size auto이며 response는 data 배열에 base64 인코딩 PNG 한 장을 담은 JSON 문서 하나입니다. transparent background와 정확한 pixel dimensions는 보장되지 않으므로 production에서 시험할 parameter가 아니라 설계 단계에서 제외하세요.",
          ),
          sharedCode(`curl ${OPENAI}/images/edits \\
  -H "Authorization: Bearer $APITOKEN_API_KEY" \\
  -F "model=gpt-image-2" \\
  -F "prompt=Replace only the background with a neutral studio backdrop; keep the product untouched" \\
  -F "image=@reference.png;type=image/png" \\
  -F "background=opaque" \\
  -F "quality=low" \\
  -F "size=auto"`),
          sharedCode(`// Response (abridged): decode data[0].b64_json into a PNG file.
{
  "data": [
    { "b64_json": "<BASE64 PNG>" }
  ]
}`),
          paragraph(
            "Decoding the payload does not finish the request. Store the request ID and the terminal usage next to the source and result files: PNG byte size is not the billing formula, the terminal usage event is the billing authority, and it is what the dashboard charge reconciles against.",
            "Декодирование payload не завершает request. Сохраните request ID и terminal usage рядом с source и result: размер PNG в байтах — не формула цены, billing authority — terminal usage event, и именно с ним сверяется charge в дашборде.",
            "解码 payload 并不代表请求结束。把 request ID 与终态 usage 和源图、结果一起保存：PNG 字节大小不是计费公式，结算权威是终态 usage event，仪表板扣费也以它为核对依据。",
            "payload decode가 request의 끝이 아닙니다. request ID와 terminal usage를 source 및 result 파일과 함께 저장하세요. PNG byte 크기는 과금 공식이 아니며 billing authority는 terminal usage event이고 dashboard charge도 이것과 대조됩니다.",
          ),
        ],
      ),
      section(
        tr(
          "The Nano Banana 2 edit: references as multimodal input",
          "Edit через Nano Banana 2: references как multimodal input",
          "Nano Banana 2 编辑：参考图即多模态输入",
          "Nano Banana 2 edit: multimodal input으로서의 reference",
        ),
        [
          paragraph(
            "Nano Banana 2 has no separate edits endpoint. An edit is an ordinary generateContent call whose parts array mixes the instruction text with one inline_data part per reference, up to 14 supported images. Because a reference is just another part, JPEG or WEBP catalog photos go in without a conversion step — a real pipeline saving when the source archive is not PNG.",
            "У Nano Banana 2 нет отдельного edits endpoint. Edit — это обычный вызов generateContent, чей parts array смешивает instruction text с одним inline_data part на reference — до 14 поддерживаемых изображений. Поскольку reference — просто ещё один part, JPEG или WEBP фото каталога отправляются без конвертации, что реально упрощает pipeline, когда архив источников не в PNG.",
            "Nano Banana 2 没有独立的编辑端点。一次编辑就是普通的 generateContent 调用：parts 数组把指令文本与每张参考图一个 inline_data part 混合，最多 14 张受支持图像。由于参考图只是另一个 part，JPEG 或 WEBP 目录照片无需转换即可传入——当源图库不是 PNG 时，这能切实简化流水线。",
            "Nano Banana 2에는 별도 edits endpoint가 없습니다. edit은 parts 배열에 instruction text와 reference당 inline_data part 하나씩을 섞는 일반 generateContent 호출이며 최대 14개 지원 이미지를 받습니다. reference가 그저 또 다른 part이므로 JPEG/WEBP 카탈로그 사진을 변환 없이 볼 수 있어 source archive가 PNG가 아닐 때 pipeline이 실제로 단순해집니다.",
          ),
          sharedCode(`curl ${ROUTER}/v1beta/models/gemini-3.1-flash-image:generateContent \\
  -H "x-goog-api-key: $APITOKEN_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d @edit-request.json

// edit-request.json
{
  "contents": [{
    "parts": [
      { "text": "Replace only the background with a neutral studio backdrop; keep the product untouched" },
      { "inline_data": { "mime_type": "image/jpeg", "data": "<BASE64 REFERENCE>" } }
    ]
  }],
  "generationConfig": {
    "responseModalities": ["TEXT", "IMAGE"],
    "imageConfig": { "imageSize": "1K", "aspectRatio": "1:1" }
  }
}`),
          paragraph(
            "generationConfig pins the output contract: responseModalities TEXT plus IMAGE, an explicit imageSize of 1K, 2K or 4K, and one of the published aspect ratios. The response carries its own parts array — the image arrives as an inlineData part with a MIME type and base64 payload, next to any text parts. The 0.5K tier is not admitted on this subscription route: requests for it are rejected locally until that capability is live-verified, so do not build a fallback on it.",
            "generationConfig фиксирует output contract: responseModalities TEXT плюс IMAGE, явный imageSize 1K, 2K или 4K и один из published aspect ratios. Response содержит собственный parts array: изображение приходит как inlineData part с MIME type и base64 payload рядом с возможными text parts. Уровень 0.5K на этом subscription route не допускается: такие запросы отклоняются локально до live-верификации capability, поэтому не стройте на него fallback.",
            "generationConfig 固定输出契约：responseModalities 为 TEXT 加 IMAGE，明确 imageSize 取 1K、2K 或 4K，并选择已发布宽高比之一。响应自带 parts 数组——图像以带 MIME type 与 base64 payload 的 inlineData part 返回，旁边可能还有文本 part。0.5K 档位在此订阅路由未开放：相关请求在该能力完成实时验证前会被本地拒绝，不要把回退方案建在它上面。",
            "generationConfig가 output contract를 고정합니다. responseModalities는 TEXT와 IMAGE, imageSize는 1K/2K/4K 중 하나, aspect ratio는 공개된 값 중 하나입니다. response는 자체 parts 배열을 담고 이미지는 MIME type과 base64 payload를 가진 inlineData part로 도착하며 text part와 함께 올 수 있습니다. 0.5K tier는 이 subscription route에서 허용되지 않으며 capability가 live 검증될 때까지 로컬에서 거부되므로 fallback를 여기에 두지 마세요.",
          ),
        ],
      ),
      section(
        tr(
          "Run edits as a validated pipeline",
          "Запускайте edits как validated pipeline",
          "把编辑当作受验证的流水线运行",
          "edit을 validated pipeline으로 실행",
        ),
        [
          steps(
            ["Normalize and inspect every reference server-side: decode the file, confirm a supported MIME type and reject empty or oversized payloads before any paid call.", "Write an edit brief that separates immutable traits — product geometry, logo, label text — from the single named change; one change per request keeps failures diagnosable.", "Choose the route whose output contract your client can decode, and never mix inlineData parsing with OpenAI Images parsing.", "Dispatch one bounded candidate per source asset; do not fan out paid variants before the first output passes review.", "Validate the delivered image — format, plausible dimensions, product identity and the absence of prohibited changes — before anything downstream sees it.", "Persist request ID, terminal usage, prompt version, source and result together; that record is both your rollback path and your cost attribution."],
            ["Нормализуйте и проверяйте каждую reference на server-side: декодируйте файл, подтвердите supported MIME type и отклоняйте empty или oversized payloads до любого paid call.", "Напишите edit brief, отделяющий immutable traits — product geometry, logo, label text — от единственного named change: одно изменение на request делает failures диагностируемыми.", "Выберите route, чей output contract умеет декодировать ваш client, и никогда не смешивайте inlineData parsing с OpenAI Images parsing.", "Отправляйте один bounded candidate на source asset; не запускайте paid variants, пока первый output не прошёл review.", "Валидируйте доставленное изображение — format, plausible dimensions, product identity и отсутствие prohibited changes — до того, как его увидит downstream.", "Сохраняйте request ID, terminal usage, prompt version, source и result вместе: эта запись — одновременно rollback path и cost attribution."],
            ["在服务端规范并检查每张参考图：解码文件，确认 MIME type 受支持，在任何付费调用前拒绝空文件或超限 payload。", "编写编辑 brief，把不可变特征——产品几何形状、logo、标签文字——与唯一的指定变更分开；每次请求只改一处，故障才可诊断。", "选择客户端能解码其输出契约的路由，绝不混用 inlineData 解析与 OpenAI Images 解析。", "每个源资产只派发一个有界候选项；首个输出通过审核前，不要铺开付费变体。", "在任何下游环节看到交付图像之前，先验证格式、合理尺寸、产品一致性以及不存在禁止改动。", "把 request ID、终态 usage、prompt 版本、源图与结果一起持久化；这份记录既是回滚路径，也是成本归因依据。"],
            ["모든 reference를 server-side에서 정규화·검사합니다. 파일을 decode하고 supported MIME type을 확인한 뒤 paid call 전에 empty/oversized payload를 거부합니다.", "edit brief에서 product geometry, logo, label text 같은 immutable trait와 단일 named change를 분리합니다. request당 변경 하나여야 failure를 진단할 수 있습니다.", "client가 decode할 수 있는 output contract의 route를 고르고 inlineData parsing과 OpenAI Images parsing을 절대 섞지 않습니다.", "source asset당 bounded candidate 하나만 본내고 첫 output이 review를 통과하기 전에 paid variant를 늘리지 않습니다.", "downstream이 보기 전에 delivered image의 format, plausible dimensions, product identity, prohibited change 부재를 검증합니다.", "request ID, terminal usage, prompt version, source, result를 함께 저장합니다. 이 기록이 rollback path이자 cost attribution입니다."],
          ),
        ],
      ),
      section(
        tr(
          "Edit cost math on published rates",
          "Расчёт стоимости edits по published rates",
          "按已发布费率演算编辑成本",
          "공개 요금으로 edit 비용 계산",
        ),
        [
          paragraph(
            "For regular B2C accounts, both models bill at exactly 50% of the official usage total. GPT Image 2 settles per token: $2.50 per 1M fresh text input, $4 per 1M fresh image input and $15 per 1M image output after the discount, with cached input recognized at one quarter of the fresh rate before the discount. Every reference you attach is billed image input, so reference count is a cost control, not only a quality control.",
            "Для обычных B2C-аккаунтов обе модели тарифицируются ровно в 50% от official usage total. GPT Image 2 считает по токенам: $2.50 за 1M fresh text input, $4 за 1M fresh image input и $15 за 1M image output после скидки, а cached input признаётся как четверть fresh rate до скидки. Каждая приложенная reference — это billable image input, поэтому число references является cost control, а не только quality control.",
            "普通 B2C 账户下，两款模型都按官方 usage 总额的五折（50%）计费。GPT Image 2 按 token 结算：折扣后 fresh text input 为每 1M $2.50，fresh image input 为每 1M $4，image output 为每 1M $15；cached input 在折扣前先按 fresh 费率的四分之一确认。每附加一张参考图都是计费 image input，因此参考图数量既是质量控制，也是成本控制。",
            "일반 B2C 계정에서 두 모델 모두 official usage total의 정확히 50%로 과금됩니다. GPT Image 2는 token 단위로 정산합니다. 할인 후 fresh text input은 1M당 $2.50, fresh image input은 1M당 $4, image output은 1M당 $15이며 cached input은 할인 전 fresh 요금의 1/4로 인정됩니다. 첨부하는 reference마다 billable image input이므로 reference 수는 quality control이면서 cost control입니다.",
          ),
          table(
            { headers: ["Worked example", "What is billed", "Regular B2C total"], rows: [["GPT Image 2 edit reporting 1,200 text-input, 4,000 image-input and 4,200 image-output tokens", "(1,200 × $2.50 + 4,000 × $4 + 4,200 × $15) / 1M", "$0.082 settled"], ["Nano Banana 2 edit at 1K", "fixed 1,120 image-output tokens + measured input legs", "$0.0336 image output + input"], ["Nano Banana 2 edit at 4K", "fixed 2,520 image-output tokens + measured input legs", "$0.0756 image output + input"]] },
            { headers: ["Пример расчёта", "Что тарифицируется", "Итог для обычного B2C"], rows: [["Edit GPT Image 2 с 1,200 text-input, 4,000 image-input и 4,200 image-output tokens", "(1,200 × $2.50 + 4,000 × $4 + 4,200 × $15) / 1M", "$0.082 settled"], ["Edit Nano Banana 2 в 1K", "фиксированные 1,120 image-output tokens + измеренный input", "$0.0336 image output + input"], ["Edit Nano Banana 2 в 4K", "фиксированные 2,520 image-output tokens + измеренный input", "$0.0756 image output + input"]] },
            { headers: ["演算示例", "计费内容", "普通 B2C 合计"], rows: [["GPT Image 2 编辑报告 1,200 text-input、4,000 image-input 与 4,200 image-output token", "(1,200 × $2.50 + 4,000 × $4 + 4,200 × $15) / 1M", "结算 $0.082"], ["Nano Banana 2 1K 编辑", "固定 1,120 image-output token + 实测输入项", "$0.0336 图像输出 + 输入"], ["Nano Banana 2 4K 编辑", "固定 2,520 image-output token + 实测输入项", "$0.0756 图像输出 + 输入"]] },
            { headers: ["계산 예시", "과금 내용", "일반 B2C 합계"], rows: [["1,200 text-input, 4,000 image-input, 4,200 image-output token을 보고한 GPT Image 2 edit", "(1,200 × $2.50 + 4,000 × $4 + 4,200 × $15) / 1M", "$0.082 정산"], ["Nano Banana 2 1K edit", "고정 1,120 image-output token + 측정된 input leg", "$0.0336 image output + input"], ["Nano Banana 2 4K edit", "고정 2,520 image-output token + 측정된 input leg", "$0.0756 image output + input"]] },
          ),
          paragraph(
            "The token counts in the first row are an illustration, not a tariff: GPT Image 2 output usage varies per request, and only terminal usage is authoritative. Nano Banana 2 is the opposite shape — the image-output leg is fixed by size ($0.0336 at 1K, $0.0504 at 2K, $0.0756 at 4K after the B2C discount), while text input, reference image input and any text or thinking output stay variable. An edit normally costs more input than a prompt-only generation; it earns that back when a good reference raises the acceptance rate and kills retries.",
            "Числа токенов в первой строке — иллюстрация, а не тариф: output usage GPT Image 2 меняется от запроса к запросу, и авторитетен только terminal usage. Nano Banana 2 устроен противоположно: image-output leg фиксирован размером ($0.0336 за 1K, $0.0504 за 2K, $0.0756 за 4K после B2C-скидки), а text input, reference image input и возможный text/thinking output остаются переменными. Edit обычно дороже prompt-only generation по input; это окупается, когда хорошая reference повышает acceptance rate и убирает retries.",
            "第一行的 token 数量是示例而非费率：GPT Image 2 的 output usage 随请求变化，只有终态 usage 具有权威性。Nano Banana 2 恰好相反——image output 项按尺寸固定（B2C 折扣后 1K 为 $0.0336，2K 为 $0.0504，4K 为 $0.0756），而 text input、参考图 image input 与可能的文本/思考输出仍是变量。编辑的输入成本通常高于纯提示词生成；当好的参考图提高验收率、消除重试时，这部分投入就能赚回来。",
            "첫 행의 token 수는 요금이 아닌 예시입니다. GPT Image 2 output usage는 요청마다 달라지며 terminal usage만이 권위입니다. Nano Banana 2는 반대 구조로 image-output leg가 크기별로 고정되고(B2C 할인 후 1K $0.0336, 2K $0.0504, 4K $0.0756) text input, reference image input, text/thinking output은 가변입니다. edit은 보통 prompt-only generation보다 input 비용이 크지만 좋은 reference가 acceptance rate를 높이고 retry를 없애면 회수됩니다.",
          ),
        ],
      ),
      section(
        tr(
          "Cost and retry discipline",
          "Дисциплина стоимости и retries",
          "成本与重试纪律",
          "비용과 retry 규율",
        ),
        [
          list(
            ["Send only references that constrain the requested edit; each one is billable image input on both routes.", "Never replay an edit automatically after an ambiguous timeout — the provider may have completed the work, and a blind retry is a second paid render. Reconcile by request ID first.", "Cap variants and attempts per source asset, and let the quality gate terminate the loop; a 50% discount halves the price of waste, it does not eliminate it.", "Give the image-editing worker its own key with a lifetime spending limit, separate from experiments, so a batch bug cannot drain the shared balance.", "Estimate before you spend: countTokens on gemini-3.1-flash-image measures input for free, and new accounts created with Google or GitHub start with a $5 welcome bonus that covers early pipeline tests."],
            ["Отправляйте только references, которые ограничивают requested edit: каждая — billable image input на обоих routes.", "Никогда не делайте automatic replay edit после ambiguous timeout: provider мог завершить работу, а blind retry — это второй paid render. Сначала проведите reconciliation по request ID.", "Ограничьте variants и attempts на source asset и позвольте quality gate завершать loop: скидка 50% делит цену waste пополам, но не устраняет его.", "Выдайте image-editing worker отдельный ключ с lifetime spending limit в стороне от experiments, чтобы batch bug не опустошил общий баланс.", "Оценивайте до траты: countTokens на gemini-3.1-flash-image бесплатно измеряет input, а новые аккаунты через Google или GitHub получают welcome bonus $5, которого хватает на ранние тесты pipeline."],
            ["只发送确实约束本次编辑的参考图；两条路由上每张都是计费 image input。", "歧义超时后绝不要自动重放编辑——提供商可能已完成工作，盲目重试就是第二次付费渲染。先按 request ID 核对。", "限制每个源资产的变体与尝试次数，让质量关卡终止循环；五折只把浪费减半，并不能消除浪费。", "为图像编辑 worker 配置带终身消费上限的独立密钥，与实验分开，避免批量 bug 耗尽共享余额。", "先估算再花钱：对 gemini-3.1-flash-image 调用 countTokens 可免费测量输入；通过 Google 或 GitHub 注册的新账户带有 $5 欢迎赠金，足以覆盖早期流水线测试。"],
            ["requested edit을 제약하는 reference만 본내세요. 두 route 모두 각각이 billable image input입니다.", "ambiguous timeout 후 edit을 자동 replay하지 마세요. provider가 작업을 완료했을 수 있고 blind retry는 두 번째 paid render입니다. 먼저 request ID로 대조하세요.", "source asset당 variant와 attempt를 제한하고 quality gate가 loop를 끝내게 하세요. 50% 할인은 낭비 비용을 반으로 줄일 뿐 없애지 않습니다.", "image-editing worker에 experiment과 분리된 lifetime spending limit이 있는 전용 key를 발급해 batch bug가 공유 잔액을 소진하지 못하게 하세요.", "쓰기 전에 추정하세요. gemini-3.1-flash-image의 countTokens는 input을 무료로 측정하고 Google/GitHub로 만든 새 계정의 $5 welcome bonus는 초기 pipeline 테스트를 충분히 커버합니다."],
          ),
        ],
      ),
    ],
    faq: [
      faq(
        tr("Which API accepts more reference images?", "Какой API принимает больше references?", "哪个 API 接受更多参考图？", "어느 API가 더 많은 reference image를 받나요?"),
        tr("Nano Banana 2 accepts up to 14 supported image inputs on its generateContent route. GPT Image 2 accepts one to five strict PNG files on its edits route.", "Nano Banana 2 принимает до 14 поддерживаемых image inputs на generateContent route. GPT Image 2 edits route принимает 1–5 строгих PNG.", "Nano Banana 2 的 generateContent 路由最多接受 14 张受支持图像输入；GPT Image 2 编辑路由接受 1–5 张严格 PNG。", "Nano Banana 2는 generateContent route에서 최대 14개 지원 image input을 받고 GPT Image 2 edit route는 strict PNG 1~5장을 받습니다."),
      ),
      faq(
        tr("Can GPT Image 2 edit JPEG or WEBP references directly?", "Может ли GPT Image 2 напрямую редактировать JPEG или WEBP references?", "GPT Image 2 能直接编辑 JPEG 或 WEBP 参考图吗？", "GPT Image 2가 JPEG/WEBP reference를 직접 편집할 수 있나요?"),
        tr("Not on the published route: edits accept strict PNG only. Convert and validate the file as PNG before the multipart upload, or route the job to Nano Banana 2, whose inlineData input accepts PNG, JPEG, WEBP, HEIC and HEIF.", "Не на published route: edits принимают только строгий PNG. Конвертируйте и проверьте файл как PNG до multipart upload либо направьте задачу в Nano Banana 2, чей inlineData input принимает PNG, JPEG, WEBP, HEIC и HEIF.", "已发布路由不支持：编辑只接受严格 PNG。应在 multipart 上传前转换并验证为 PNG，或把任务路由到 Nano Banana 2——其 inlineData 输入接受 PNG、JPEG、WEBP、HEIC 与 HEIF。", "공개 route에서는 안 됩니다. edit은 strict PNG만 받습니다. multipart upload 전 PNG로 변환·검증하거나 inlineData input이 PNG, JPEG, WEBP, HEIC, HEIF를 받는 Nano Banana 2로 작업을 본내세요."),
      ),
      faq(
        tr("Do edits cost more than prompt-only generation?", "Edits дороже prompt-only generation?", "编辑是否比纯提示词生成更贵？", "edit이 prompt-only generation보다 비싼가요?"),
        tr("They add billable image input, so an otherwise comparable edit normally has more input cost. It can still be cheaper per accepted asset when references raise the acceptance rate and remove retries — measure settled cost per accepted asset, not per request.", "Они добавляют billable image input, поэтому comparable edit обычно дороже по input. Но он может быть дешевле per accepted asset, если references повышают acceptance rate и убирают retries: считайте settled cost на принятый ассет, а не на request.", "编辑会增加计费 image input，因此可比编辑的输入成本通常更高；但当参考图提高验收率、消除重试时，每个验收资产的成本仍可能更低——应按验收资产的结算成本衡量，而不是按请求。", "billable image input이 추가되어 comparable edit은 보통 input 비용이 더 큽니다. 다만 reference가 acceptance rate를 높이고 retry를 없애면 accepted asset당 비용은 더 낮을 수 있으므로 request당이 아니라 accepted asset당 settled cost를 측정하세요."),
      ),
      faq(
        tr("Is it safe to retry an edit after a timeout?", "Безопасно ли retry edit после timeout?", "超时后重试编辑安全吗？", "timeout 후 edit을 retry해도 안전한가요?"),
        tr("Only when you can prove the prior attempt was not accepted. An ambiguous timeout may hide completed provider work; preserve the request ID and reconcile before another paid attempt, otherwise the retry is a second paid render of the same job.", "Только если доказано, что prior attempt не был принят. Ambiguous timeout может скрывать завершённую работу provider; сохраните request ID и проведите reconciliation до нового paid attempt — иначе retry станет вторым paid render той же задачи.", "只有能证明前一次尝试未被接受时才安全。歧义超时可能掩盖已完成的提供商工作；应保留 request ID 并先核对，否则重试就是对同一任务的第二次付费渲染。", "prior attempt가 accepted되지 않았음을 증명할 수 있을 때만 안전합니다. ambiguous timeout은 완료된 provider 작업을 숨길 수 있으므로 request ID를 보존하고 다음 paid attempt 전에 대조하세요. 그렇지 않으면 retry는 같은 작업의 두 번째 paid render가 됩니다."),
      ),
      faq(
        tr("What does the editing response look like on each route?", "Как выглядит response редактирования на каждом route?", "两条路由的编辑响应分别是什么样？", "각 route의 edit response는 어떻게 생겼나요?"),
        tr("GPT Image 2 returns one JSON document with a single base64 PNG in data[0].b64_json. Nano Banana 2 returns the Gemini candidates structure, where the image is an inlineData part with a MIME type and base64 payload. Neither route hands you a hosted URL: decode, validate and store the bytes yourself.", "GPT Image 2 возвращает один JSON-документ с единственным base64 PNG в data[0].b64_json. Nano Banana 2 возвращает Gemini candidates structure, где изображение — inlineData part с MIME type и base64 payload. Ни один route не даёт hosted URL: декодируйте, валидируйте и сохраняйте байты самостоятельно.", "GPT Image 2 返回单个 JSON 文档，data[0].b64_json 中是一张 base64 PNG。Nano Banana 2 返回 Gemini candidates 结构，图像是带 MIME type 与 base64 payload 的 inlineData part。两条路由都不提供托管 URL：需自行解码、验证并存储字节。", "GPT Image 2는 data[0].b64_json에 base64 PNG 한 장을 담은 JSON 문서 하나를 반환합니다. Nano Banana 2는 Gemini candidates 구조를 반환하며 이미지는 MIME type과 base64 payload를 가진 inlineData part입니다. 어느 route도 hosted URL을 주지 않으므로 bytes를 직접 decode, validate, 저장하세요."),
      ),
      faq(
        tr("How can I test the edit routes cheaply?", "Как дёшево протестировать edit routes?", "如何低成本测试编辑路由？", "edit route를 저렴하게 테스트하려면?"),
        tr("Register with Google or GitHub to receive the $5 welcome bonus, run GPT Image 2 at its published low/auto profile, and use countTokens to preview Nano Banana 2 input before any image is rendered. Validate the whole pipeline at 1K before reserving 2K or 4K output.", "Зарегистрируйтесь через Google или GitHub и получите welcome bonus $5, запускайте GPT Image 2 в published low/auto profile и используйте countTokens, чтобы оценить input Nano Banana 2 до рендера изображения. Проверяйте весь pipeline на 1K, прежде чем резервировать 2K или 4K output.", "通过 Google 或 GitHub 注册即可获得 $5 欢迎赠金；用已发布的 low/auto 配置运行 GPT Image 2，并用 countTokens 在渲染图像前预估 Nano Banana 2 的输入。先用 1K 验证整条流水线，再考虑 2K 或 4K 输出。", "Google 또는 GitHub로 가입해 $5 welcome bonus를 받고 GPT Image 2는 공개된 low/auto profile로 실행하며 countTokens로 이미지 렌더링 전 Nano Banana 2 input을 미리 확인하세요. 2K/4K output을 예약하기 전에 1K로 전체 pipeline을 검증하세요."),
      ),
    ],
  };
