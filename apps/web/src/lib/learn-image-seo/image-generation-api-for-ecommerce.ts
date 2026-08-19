import type { ImageSeoSpec } from "./shared";
import { faq, list, note, paragraph, section, sharedCode, steps, table, tr, ROUTER, OPENAI } from "./shared";

export const spec: ImageSeoSpec = {
    slug: "image-generation-api-for-ecommerce",
    cluster: "integrate",
    related: ["image-editing-api-guide", "batch-image-generation-api", "nano-banana-2-vs-gpt-image-2", "image-generation-api-pricing"],
    title: tr(
      "Image Generation API for Ecommerce Product Photos",
      "API генерации изображений для ecommerce-каталога",
      "电商产品图片生成 API",
      "이커머스 상품 사진 생성 API",
    ),
    h1: tr(
      "An image generation API workflow for ecommerce catalogs",
      "Workflow генерации изображений для ecommerce-каталога через API",
      "面向电商目录的图像生成 API 工作流",
      "이커머스 카탈로그를 위한 이미지 생성 API 워크플로",
    ),
    description: tr(
      "Ecommerce image generation API workflow: product photos with Nano Banana 2 or GPT Image 2 — identity-safe edits, batch controls, cost math, SEO.",
      "API генерации изображений для ecommerce: товарные фото через Nano Banana 2 или GPT Image 2 — identity-safe edits, batch-контроль, расчёт цены, SEO.",
      "用一个 API 密钥通过 Nano Banana 2 或 GPT Image 2 生产电商产品图：模型路由、真实请求结构、保真编辑、批量控制、成本计算与店面 SEO。",
      "하나의 API key로 Nano Banana 2 또는 GPT Image 2를 사용해 ecommerce 상품 이미지를 만듭니다: 모델 routing, 실제 request shape, identity-safe 편집, batch control, 비용 계산, storefront SEO.",
    ),
    keywords: tr(
      ["ecommerce image generation api", "product image api", "ai product photography api", "product background generator api", "catalog image automation", "ai ecommerce images"],
      ["api генерации изображений для ecommerce", "api картинок товара", "ai фото товара api", "api генератора фона товара", "автоматизация изображений каталога", "ai картинки для интернет магазина"],
      ["电商图像生成 api", "产品图像 api", "ai 产品摄影 api", "产品背景生成 api", "目录图像自动化", "ai 电商图像"],
      ["ecommerce 이미지 생성 api", "상품 이미지 api", "ai 상품 사진 api", "상품 배경 생성 api", "카탈로그 이미지 자동화", "ai ecommerce 이미지"],
    ),
    dek: tr(
      "Product imagery is an identity-preservation problem before it is a prompt-writing problem. Lock the product traits, permit only named scene changes, validate every output, and optimize cost only after the acceptance gate is reliable.",
      "Product imagery — сначала задача сохранения identity, затем prompt writing. Зафиксируйте свойства товара, разрешите только названные изменения сцены, валидируйте каждый output и оптимизируйте цену после надёжного acceptance gate.",
      "产品图像首先是身份保持问题，其次才是提示词问题。锁定产品特征，只允许明确场景变化，验证每个输出，并在验收门可靠后优化成本。",
      "product imagery는 prompt 작성 전에 identity 보존 문제입니다. 상품 특성을 고정하고 명시된 장면 변경만 허용하며 모든 output을 검증한 뒤 acceptance gate가 안정적일 때 비용을 최적화하세요.",
    ),
    sections: [
      section(
        tr("Match each catalog job to the right model", "Подберите модель под каждую задачу каталога", "为每个目录任务匹配正确的模型", "각 카탈로그 작업에 맞는 모델 매칭"),
        [
          paragraph(
            "You can produce catalog-ready product images programmatically with two live models behind one prepaid apiToken.sale key: Nano Banana 2 (gemini-3.1-flash-image) on the native Gemini route and GPT Image 2 (gpt-image-2) on the OpenAI Images route. The right choice is per job, not per store: Nano Banana 2 gives explicit 1K/2K/4K sizes, broad aspect-ratio control and up to 14 reference inputs, while GPT Image 2 gives a strict PNG edit flow with one to five references for teams already on an OpenAI Images client.",
            "Каталожные product images можно производить программно через две live-модели за одним prepaid-ключом apiToken.sale: Nano Banana 2 (gemini-3.1-flash-image) на native Gemini route и GPT Image 2 (gpt-image-2) на OpenAI Images route. Выбор делается под задачу, а не под магазин: Nano Banana 2 даёт явные размеры 1K/2K/4K, широкий контроль aspect ratio и до 14 reference inputs, а GPT Image 2 — строгий PNG edit flow с 1–5 references для команд, уже работающих на OpenAI Images client.",
            "你可以通过一个 apiToken.sale 预付密钥，以编程方式生产可上架的产品图：Nano Banana 2（gemini-3.1-flash-image）走原生 Gemini 路由，GPT Image 2（gpt-image-2）走 OpenAI Images 路由。选择应按任务而非按店铺决定：Nano Banana 2 提供明确的 1K/2K/4K 尺寸、丰富的宽高比控制和最多 14 张参考图；GPT Image 2 则为已有 OpenAI Images 客户端的团队提供 1–5 张参考图的严格 PNG 编辑流程。",
            "하나의 prepaid apiToken.sale key 뒤의 두 live 모델, 즉 native Gemini route의 Nano Banana 2(gemini-3.1-flash-image)와 OpenAI Images route의 GPT Image 2(gpt-image-2)로 카탈로그용 상품 이미지를 프로그래밍 방식으로 생산할 수 있습니다. 선택은 스토어 단위가 아니라 작업 단위입니다. Nano Banana 2는 명시적 1K/2K/4K 크기, 다양한 aspect ratio 제어, 최대 14개 reference input을 제공하고 GPT Image 2는 기존 OpenAI Images client 팀을 위한 1~5개 reference의 strict PNG edit flow를 제공합니다.",
          ),
          table(
            { headers: ["Job", "Recommended starting route", "Mandatory check"], rows: [["New lifestyle scene for a hero shot", "Nano Banana 2 1K with explicit aspect ratio", "Product identity and packaging text survive"], ["Strict background swap on packshot PNG", "GPT Image 2 /v1/images/edits", "No product geometry change"], ["One SKU, many reference angles", "Nano Banana 2 with the full reference set", "Consistency of the product across views"], ["Existing OpenAI media pipeline", "GPT Image 2", "Opaque background and auto size acceptable"], ["Whole-catalog seasonal refresh", "Batch queue over either model", "Settled cost per accepted SKU asset"]] },
            { headers: ["Задача", "Стартовый route", "Обязательная проверка"], rows: [["Новая lifestyle scene для hero shot", "Nano Banana 2 1K с явным aspect ratio", "Product identity и текст упаковки сохранены"], ["Строгая замена фона на packshot PNG", "GPT Image 2 /v1/images/edits", "Геометрия товара не изменилась"], ["Один SKU, много reference angles", "Nano Banana 2 с полным набором references", "Консистентность товара между видами"], ["Готовый OpenAI media pipeline", "GPT Image 2", "Opaque background и auto size приемлемы"], ["Сезонное обновление всего каталога", "Batch queue поверх любой модели", "Settled cost на принятый SKU asset"]] },
            { headers: ["任务", "建议起始路由", "强制检查"], rows: [["主图的新生活方式场景", "Nano Banana 2 1K + 明确宽高比", "产品一致性与包装文字保持"], ["白底 PNG 的严格背景替换", "GPT Image 2 /v1/images/edits", "产品几何形状不变"], ["单个 SKU 的多角度参考", "Nano Banana 2 + 完整参考集", "跨视角的产品一致性"], ["已有 OpenAI 媒体流水线", "GPT Image 2", "可接受不透明背景与 auto 尺寸"], ["全目录季节性更新", "任一模型上的 batch 队列", "每个验收 SKU 资产的结算成本"]] },
            { headers: ["작업", "권장 시작 route", "필수 검사"], rows: [["hero shot용 새 lifestyle scene", "명시적 aspect ratio의 Nano Banana 2 1K", "product identity와 packaging text 유지"], ["packshot PNG의 strict 배경 교체", "GPT Image 2 /v1/images/edits", "product geometry 변경 없음"], ["한 SKU의 다각도 reference", "전체 reference set의 Nano Banana 2", "view 간 상품 일관성"], ["기존 OpenAI media pipeline", "GPT Image 2", "opaque background와 auto size 수용 가능"], ["전체 카탈로그 시즌 갱신", "어느 모델이든 batch queue", "accepted SKU asset당 settled 비용"]] },
          ),
          note(
            "GPT Image 2 does not publish transparent background or exact dimensions on this route. If a transparent cutout or a fixed pixel size is a hard storefront requirement, reject that route at design time instead of repairing every output later.",
            "GPT Image 2 на этом route не обещает transparent background и exact dimensions. Если прозрачный cutout или фиксированный pixel size — жёсткое требование витрины, отклоните route на этапе дизайна, а не чините каждый output потом.",
            "该路由上的 GPT Image 2 不承诺透明背景或准确尺寸。如果透明抠图或固定像素尺寸是店面的硬性要求，应在设计阶段排除该路由，而不是事后修复每个输出。",
            "GPT Image 2는 이 route에서 transparent background나 exact dimensions를 약속하지 않습니다. 투명 cutout이나 고정 pixel size가 storefront의 필수 요건이면 모든 output을 나중에 고치지 말고 설계 단계에서 route를 제외하세요.",
          ),
          tr(
            { type: "link", text: "Editing route contracts: references, formats, output shapes", href: "/docs/learn/image-editing-api-guide" },
            { type: "link", text: "Контракты маршрутов редактирования: references, форматы, формы output", href: "/docs/learn/image-editing-api-guide" },
            { type: "link", text: "编辑路由契约：参考图、格式与输出形态", href: "/docs/learn/image-editing-api-guide" },
            { type: "link", text: "편집 route 계약: reference, 형식, output 형태", href: "/docs/learn/image-editing-api-guide" },
          ),
        ],
      ),
      section(
        tr("Two wire protocols behind one prepaid key", "Два wire protocol за одним prepaid-ключом", "一个预付密钥背后的两种线路协议", "하나의 prepaid key 뒤의 두 wire protocol"),
        [
          paragraph(
            "The same key works for both routes, but the request shapes are not interchangeable. Nano Banana 2 is called with the Gemini generateContent shape and the x-goog-api-key header; the response carries the picture as a base64 inlineData image part. GPT Image 2 uses the OpenAI Images endpoints with Authorization: Bearer and returns one non-streaming base64 PNG. Size and quality controls differ too: imageConfig.imageSize (1K/2K/4K) plus aspectRatio on the Gemini side, the published background/quality/size values opaque/low/auto on the OpenAI side.",
            "Один и тот же ключ работает для обоих routes, но request shapes невзаимозаменяемы. Nano Banana 2 вызывается через Gemini generateContent с заголовком x-goog-api-key; ответ несёт картинку как base64 inlineData image part. GPT Image 2 использует OpenAI Images endpoints с Authorization: Bearer и возвращает один non-streaming base64 PNG. Controls размера и качества тоже разные: imageConfig.imageSize (1K/2K/4K) и aspectRatio на стороне Gemini, опубликованные значения background/quality/size — opaque/low/auto — на стороне OpenAI.",
            "同一个密钥可用于两条路由，但请求结构不可互换。Nano Banana 2 使用 Gemini generateContent 结构与 x-goog-api-key 请求头调用，响应以 base64 inlineData 图像 part 返回图片。GPT Image 2 使用 OpenAI Images 端点与 Authorization: Bearer，返回单张非流式 base64 PNG。尺寸与质量控制也不同：Gemini 侧是 imageConfig.imageSize（1K/2K/4K）加 aspectRatio，OpenAI 侧是已发布的 background/quality/size 取值 opaque/low/auto。",
            "같은 key가 두 route 모두에서 동작하지만 request shape는 교환할 수 없습니다. Nano Banana 2는 Gemini generateContent 형식과 x-goog-api-key 헤더로 호출되며 응답은 base64 inlineData image part로 그림을 전달합니다. GPT Image 2는 Authorization: Bearer와 함께 OpenAI Images endpoint를 쓰고 non-streaming base64 PNG 한 장을 반환합니다. 크기·품질 control도 다릅니다. Gemini 쪽은 imageConfig.imageSize(1K/2K/4K)와 aspectRatio, OpenAI 쪽은 공개된 background/quality/size 값 opaque/low/auto입니다.",
          ),
          sharedCode(`# Nano Banana 2 — native Gemini route, explicit size and ratio
curl ${ROUTER}/v1beta/models/gemini-3.1-flash-image:generateContent \\
  -H "x-goog-api-key: $APITOKEN_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{"contents":[{"parts":[{"text":"Place SKU-1042 on a warm kitchen counter, soft daylight"}]}],"generationConfig":{"responseModalities":["TEXT","IMAGE"],"imageConfig":{"imageSize":"1K","aspectRatio":"4:5"}}}'

# GPT Image 2 — OpenAI Images route, published controls only
curl ${OPENAI}/images/generations \\
  -H "Authorization: Bearer $APITOKEN_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{"model":"gpt-image-2","prompt":"Studio packshot of a matte black bottle, seamless grey background","background":"opaque","quality":"low","size":"auto"}'`),
          paragraph(
            "Getting started is cheap by design: new B2C accounts created with Google or GitHub receive $5 of platform bonus credit (valid on Claude, GPT and Gemini models; email-and-password accounts are not eligible), and balance top-ups accept bank card or cryptocurrency. For a catalog worker, issue a dedicated key with a lifetime spending limit and an expiration date so a runaway batch cannot drain the whole account.",
            "Старт специально дешёвый: новые B2C-аккаунты через Google или GitHub получают бонус $5 на баланс платформы (действует для моделей Claude, GPT и Gemini; аккаунты по email и паролю его не получают), а пополнение принимает банковскую карту или криптовалюту. Для catalog worker выпустите отдельный ключ с общим лимитом расходов и датой истечения, чтобы runaway batch не опустошил весь аккаунт.",
            "入门成本刻意压得很低：通过 Google 或 GitHub 创建的新 B2C 账户可获得 $5 平台奖励金（适用于 Claude、GPT 和 Gemini 模型；邮箱密码账户不参与），余额充值支持银行卡或加密货币。为目录 worker 签发一把带终身消费上限和到期日期的独立密钥，防止失控的批量任务耗尽整个账户。",
            "시작 비용은 의도적으로 저렴합니다. Google이나 GitHub로 만든 신규 B2C 계정은 $5 플랫폼 본너스 크레딧을 받고(Claude, GPT, Gemini 모델에 유효하며 이메일·비밀번호 계정은 제외), 잔액 충전은 은행 카드나 암호화폐로 가능합니다. 카탈로그 worker에는 평생 누적 지출 한도와 만료일이 있는 전용 key를 발급해 runaway batch가 전체 계정을 소진하지 못하게 하세요.",
          ),
        ],
      ),
      section(
        tr("A production workflow that protects the product", "Production workflow, защищающий товар", "保护产品一致性的生产工作流", "상품을 보호하는 production workflow"),
        [
          steps(
            ["Normalize source photography and assign an immutable SKU/asset version to every input file.", "Write the brief as allowed changes plus protected traits: silhouette, color, logo, labels and included accessories.", "Generate one bounded candidate, then validate product identity, rendered text, artifacts, composition and policy compliance.", "Store the original, output, prompt version, model, request ID, terminal usage and reviewer verdict as one durable record.", "Publish only an optimized web derivative; never serve the API's base64 payload directly as a storefront asset."],
            ["Нормализуйте исходную фотосъёмку и присвойте каждому входному файлу immutable SKU/asset version.", "Запишите brief как allowed changes плюс protected traits: silhouette, color, logo, labels и комплектные accessories.", "Сгенерируйте один bounded candidate и проверьте product identity, отрисованный текст, artifacts, composition и policy compliance.", "Сохраните original, output, prompt version, model, request ID, terminal usage и reviewer verdict как одну durable запись.", "Публикуйте только оптимизированную web derivative; base64 payload из API нельзя отдавать на витрину напрямую."],
            ["规范源产品照片，为每个输入文件分配不可变的 SKU/asset 版本。", "把 brief 写成允许的变更加受保护特征：轮廓、颜色、logo、标签与随附配件。", "生成一个有界候选项，然后验证产品一致性、渲染文字、瑕疵、构图与合规。", "把原图、输出、prompt 版本、模型、request ID、终态 usage 与审核结论存为一条持久记录。", "只发布优化后的 web derivative；绝不要把 API 的 base64 payload 直接当作店面资产。"],
            ["소스 촬영물을 정규화하고 모든 입력 파일에 immutable SKU/asset version을 부여합니다.", "brief를 허용 변경과 protected trait(silhouette, color, logo, label, 포함 accessory)로 작성합니다.", "bounded candidate 하나를 생성하고 product identity, 렌더링된 text, artifact, composition, policy compliance를 검증합니다.", "original, output, prompt version, model, request ID, terminal usage, reviewer verdict를 하나의 durable record로 저장합니다.", "최적화된 web derivative만 게시하고 API의 base64 payload를 storefront asset으로 직접 제공하지 않습니다."],
          ),
          sharedCode(`PRODUCT: SKU-1042, matte black bottle, silver cap, logo unchanged
ALLOWED: replace the background with a warm kitchen scene
PROTECTED: silhouette, cap shape, logo spelling, label colors
OUTPUT: one centered catalog hero image; no added text or accessories`),
          note(
            "A prompt is an instruction, not a guarantee. Validation against the protected traits is what keeps a regenerated background from quietly becoming a different product.",
            "Prompt — это инструкция, а не гарантия. Только validation по protected traits не даёт перегенерированному фону незаметно превратиться в другой товар.",
            "提示词是指令而非保证。只有对照受保护特征进行验证，才能防止重新生成的背景悄悄变成另一件产品。",
            "prompt는 지시일 뿐 보장이 아닙니다. protected trait에 대한 validation만이 재생성된 배경이 조용히 다른 상품으로 바뀌는 것을 막습니다.",
          ),
        ],
      ),
      section(
        tr("Cost math for a real catalog run", "Расчёт цены реального прогона каталога", "真实目录批次的成本计算", "실제 카탈로그 실행의 비용 계산"),
        [
          paragraph(
            "Nano Banana 2 has a fixed image-output leg per size: 1K bills 1,120 image tokens, which is $0.0672 official and $0.0336 for a regular B2C account after the 50% discount; 2K is $0.0504 and 4K is $0.0756 on the same terms. A seasonal refresh of 500 SKUs with two 1K candidates each produces 1,000 images, so the image-output legs settle at $33.60 for regular B2C, plus bounded prompt and reference input. If validation accepts 700 of those candidates, the image-leg cost per accepted asset is $33.60 ÷ 700 ≈ $0.048 — that is the number to compare against a reshoot budget, not the raw request count.",
            "У Nano Banana 2 фиксированная image-output составляющая на размер: 1K — это 1 120 image tokens, $0.0672 официально и $0.0336 для обычного B2C после скидки 50%; 2K — $0.0504 и 4K — $0.0756 на тех же условиях. Сезонное обновление 500 SKU по два кандидата 1K даёт 1 000 изображений, то есть image-output legs осядут в $33.60 для обычного B2C плюс ограниченный prompt и reference input. Если validation примет 700 кандидатов, цена image leg на принятый ассет — $33.60 ÷ 700 ≈ $0.048. Именно её сравнивают с бюджетом пересъёмки, а не количество запросов.",
            "Nano Banana 2 的图像输出项按尺寸固定：1K 计 1,120 个 image token，官方 $0.0672，普通 B2C 五折后 $0.0336；同样条件下 2K 为 $0.0504，4K 为 $0.0756。500 个 SKU 的季节性更新、每个生成两张 1K 候选，共 1,000 张图，普通 B2C 的图像输出项合计 $33.60，另有有界的提示词与参考图输入。若验证通过其中 700 张，每个验收资产的图像项成本为 $33.60 ÷ 700 ≈ $0.048——应该拿这个数字而不是原始请求数去对比重拍预算。",
            "Nano Banana 2는 크기별 고정 image-output leg가 있습니다. 1K는 1,120 image token으로 공식 $0.0672, 일반 B2C는 50% 할인 후 $0.0336입니다. 같은 조건에서 2K는 $0.0504, 4K는 $0.0756입니다. SKU 500개를 각각 1K candidate 두 장씩 시즌 갱신하면 이미지 1,000장이 나오고 일반 B2C의 image-output leg는 bounded prompt와 reference input을 더해 $33.60에 정산됩니다. validation이 700장을 수용하면 accepted asset당 image-leg 비용은 $33.60 ÷ 700 ≈ $0.048입니다. 원시 request 수가 아니라 이 숫자를 재촬영 예산과 비교하세요.",
          ),
          paragraph(
            "GPT Image 2 has no honest per-picture list price: billing combines terminal text-input, image-input, cached-input and image-output usage. A worked example with round numbers — if terminal usage reports 500 fresh text tokens and 4,000 image-output tokens, the official total is 500 × $5/M + 4,000 × $30/M = $0.1225, and a regular B2C account pays exactly half, $0.06125. References add image-input at $8/M official ($4/M after the discount), so an edit brief should send only the files the scene actually needs.",
            "У GPT Image 2 нет честной прейскурантной цены за картинку: billing складывает terminal text-input, image-input, cached-input и image-output usage. Счётный пример с круглыми числами: если terminal usage показывает 500 fresh text tokens и 4 000 image-output tokens, официальный итог — 500 × $5/M + 4 000 × $30/M = $0.1225, а обычный B2C платит ровно половину, $0.06125. References добавляют image-input по $8/M официально ($4/M после скидки), поэтому edit brief должен отправлять только файлы, реально нужные сцене.",
            "GPT Image 2 没有诚实的单张标价：结算由终态 text-input、image-input、cached-input 与 image-output usage 合成。一个取整的演算示例——若终态 usage 报告 500 个 fresh text token 与 4,000 个 image-output token，官方总额为 500 × $5/M + 4,000 × $30/M = $0.1225，普通 B2C 恰好支付一半，即 $0.06125。参考图按官方 $8/M（折后 $4/M）增加 image-input，因此编辑 brief 只应发送场景真正需要的文件。",
            "GPT Image 2에는 정직한 이미지당 정가가 없습니다. billing은 terminal text-input, image-input, cached-input, image-output usage를 합산합니다. 반올림 수치의 계산 예로 terminal usage가 fresh text token 500개와 image-output token 4,000개를 보고하면 공식 합계는 500 × $5/M + 4,000 × $30/M = $0.1225이고 일반 B2C는 정확히 절반인 $0.06125를 냅니다. reference는 공식 $8/M(할인 후 $4/M)의 image-input을 더하므로 edit brief는 장면에 실제로 필요한 파일만 보내야 합니다.",
          ),
          note(
            "For regular B2C, the 50% discount applies to the exact official generation or edit usage. Conversion, CDN storage, human review and rejected assets remain your application costs and belong in the ecommerce business case.",
            "Для обычного B2C скидка 50% применяется к exact official generation/edit usage. Conversion, CDN storage, human review и rejected assets остаются application costs и входят в ecommerce business case.",
            "普通 B2C 的五折适用于准确官方生成或编辑 usage；转换、CDN 存储、人工审核与被拒资产仍属于应用成本，应计入电商商业模型。",
            "일반 B2C의 50% 할인은 exact official generation/edit usage에 적용됩니다. conversion, CDN storage, human review, rejected asset은 application cost로 ecommerce business case에 포함해야 합니다.",
          ),
          tr(
            { type: "link", text: "Budget-safe batch pipeline for whole-catalog runs", href: "/docs/learn/batch-image-generation-api" },
            { type: "link", text: "Batch-пайплайн с защитой бюджета для прогонов всего каталога", href: "/docs/learn/batch-image-generation-api" },
            { type: "link", text: "面向全目录批次的预算安全流水线", href: "/docs/learn/batch-image-generation-api" },
            { type: "link", text: "전체 카탈로그 run을 위한 예산 안전 batch pipeline", href: "/docs/learn/batch-image-generation-api" },
          ),
        ],
      ),
      section(
        tr("Ship images that help storefront SEO", "Публикуйте изображения, полезные для SEO витрины", "发布有利于店面 SEO 的图像", "storefront SEO에 도움이 되는 이미지 게시"),
        [
          list(
            ["Track settled cost per accepted SKU image, not request count; retries and rejects are part of acquisition cost.", "Start at the smallest delivery resolution (1K) and promote only hero assets that fail a delivery-detail check.", "Use descriptive filenames and truthful alt text describing the actual product and scene; never stuff model names or keyword lists into alt text.", "Encode responsive WebP/AVIF derivatives, keep a lossless master, and publish explicit width/height to avoid layout shift.", "Keep generated-image provenance and review evidence out of public metadata when it contains internal IDs or prompts."],
            ["Считайте settled cost на принятый SKU image, а не request count: retries и rejects — часть acquisition cost.", "Начинайте с минимального delivery resolution (1K) и повышайте только hero assets, не прошедшие проверку детализации.", "Используйте descriptive filenames и правдивый alt text про реальный товар и сцену; не набивайте alt названиями моделей или списками ключевых слов.", "Кодируйте responsive WebP/AVIF derivatives, храните lossless master и публикуйте явные width/height против layout shift.", "Держите provenance и review evidence вне public metadata, если там internal IDs или prompts."],
            ["跟踪每个验收 SKU 图像的结算成本，而非请求数；重试与被拒都属于获取成本。", "从最小交付分辨率（1K）开始，仅升级未通过细节检查的 hero 资产。", "使用描述性文件名与如实描述产品和场景的 alt 文本；绝不要把模型名称或关键词堆砌进 alt。", "编码响应式 WebP/AVIF 衍生图，保留无损 master，并发布明确的 width/height 以避免布局偏移。", "若来源与审核证据包含内部 ID 或提示词，应将其排除在公开 metadata 之外。"],
            ["request 수가 아닌 accepted SKU image당 settled 비용을 추적하세요. retry와 reject도 acquisition cost의 일부입니다.", "가장 작은 delivery resolution(1K)으로 시작하고 delivery-detail 검사를 통과하지 못한 hero asset만 승격합니다.", "실제 상품과 장면을 설명하는 descriptive filename과 정직한 alt text를 쓰고 model name이나 keyword 목록을 alt에 채우지 마세요.", "responsive WebP/AVIF derivative를 인코딩하고 lossless master를 유지하며 layout shift 방지를 위해 명시적 width/height를 게시합니다.", "provenance와 review evidence에 internal ID나 prompt가 있으면 public metadata에서 제외합니다."],
          ),
        ],
      ),
    ],
    faq: [
      faq(
        tr("Which model should an ecommerce team start with for product images?", "С какой модели ecommerce-команде начать product images?", "电商团队制作产品图应从哪款模型开始？", "ecommerce 팀은 상품 이미지에 어떤 모델부터 시작해야 하나요?"),
        tr("Run a small eval on your own SKUs. Nano Banana 2 is the stronger starting point when you need explicit aspect ratios, 1K/2K/4K sizes or many reference angles; GPT Image 2 fits strict PNG background edits and teams with an existing OpenAI Images client. Pin the winner per asset class rather than choosing one model for the whole catalog.", "Прогоните небольшой eval на своих SKU. Nano Banana 2 — более сильная стартовая точка, когда нужны явные aspect ratios, размеры 1K/2K/4K или много reference angles; GPT Image 2 подходит для строгих PNG background edits и команд с готовым OpenAI Images client. Закрепляйте победителя за классом ассетов, а не выбирайте одну модель на весь каталог.", "用自己的 SKU 做一个小型评测。需要明确宽高比、1K/2K/4K 尺寸或多角度参考时，Nano Banana 2 是更强的起点；严格 PNG 背景编辑与已有 OpenAI Images 客户端的团队适合 GPT Image 2。按资产类别固定胜出模型，而不是为整个目录只选一款。", "자체 SKU로 작은 eval을 실행하세요. 명시적 aspect ratio, 1K/2K/4K 크기, 많은 reference angle이 필요하면 Nano Banana 2가 더 강한 출발점이고 strict PNG background edit과 기존 OpenAI Images client를 가진 팀에는 GPT Image 2가 맞습니다. 전체 카탈로그에 한 모델을 고르지 말고 asset class별 승자를 고정하세요."),
      ),
      faq(
        tr("Can I generate transparent product cutouts with GPT Image 2 here?", "Можно ли здесь делать transparent cutouts через GPT Image 2?", "本站能用 GPT Image 2 生成透明产品抠图吗？", "여기 GPT Image 2로 투명 상품 cutout을 만들 수 있나요?"),
        tr("No. The proved route supports an opaque background only, with the published background/quality/size values opaque/low/auto. Use a validated downstream segmentation step, or pick a workflow whose live contract supports the background your storefront requires.", "Нет. Подтверждённый route поддерживает только opaque background с опубликованными значениями background/quality/size — opaque/low/auto. Используйте проверенный downstream segmentation step или workflow, чей live contract поддерживает нужный витрине фон.", "不能。已验证路由仅支持不透明背景，已发布的 background/quality/size 取值为 opaque/low/auto。请使用经过验证的下游分割步骤，或选择实时契约满足店面背景要求的工作流。", "아닙니다. 검증된 route는 공개된 background/quality/size 값 opaque/low/auto로 opaque background만 지원합니다. 검증된 downstream segmentation 단계를 쓰거나 storefront가 요구하는 background를 지원하는 live contract workflow를 선택하세요."),
      ),
      faq(
        tr("How do I stop the model from changing the product itself?", "Как не дать модели изменить сам товар?", "如何防止模型改动产品本身？", "모델이 상품 자체를 바꾸지 못하게 하려면?"),
        tr("Declare protected traits in the brief — silhouette, colors, logo spelling, label text — provide clean references, change one named element at a time, and reject any candidate that fails product-identity validation. The prompt narrows the search space; only the acceptance gate protects the catalog.", "Опишите в brief protected traits — silhouette, colors, logo spelling, label text, — дайте чистые references, меняйте по одному названному элементу за раз и отклоняйте любой кандидат, не прошедший product-identity validation. Prompt сужает пространство поиска; каталог защищает только acceptance gate.", "在 brief 中声明受保护特征——轮廓、颜色、logo 拼写、标签文字——提供干净的参考图，每次只改一个具名元素，并拒绝任何未通过产品一致性验证的候选。提示词只能缩小搜索空间，保护目录的是验收门。", "brief에 protected trait(silhouette, color, logo spelling, label text)를 선언하고 깨끗한 reference를 제공하며 한 번에 named element 하나만 바꾸고 product-identity validation에 실패한 candidate는 거부하세요. prompt는 검색 공간을 좁힐 뿐이며 카탈로그를 지키는 것은 acceptance gate입니다."),
      ),
      faq(
        tr("What does it cost to re-shoot a 1,000-image catalog?", "Сколько стоит пересъёмка каталога на 1 000 изображений?", "重拍 1,000 张图的目录要多少钱？", "이미지 1,000장 카탈로그 재촬영 비용은?"),
        tr("With Nano Banana 2 at 1K, one thousand image-output legs settle at $33.60 for a regular B2C account after the 50% discount, plus bounded input. Divide the settled total by the number of accepted assets to get the real figure; a 70% acceptance rate puts the image-leg cost near $0.048 per accepted SKU image.", "На Nano Banana 2 в 1K тысяча image-output legs осядет в $33.60 для обычного B2C после скидки 50% плюс ограниченный input. Разделите settled total на число принятых ассетов: при acceptance rate 70% цена image leg составит около $0.048 на принятый SKU image.", "以 1K 的 Nano Banana 2 计算，一千个图像输出项在普通 B2C 五折后合计 $33.60，另有有界输入。把结算总额除以验收资产数才是真实数字：验收率 70% 时，每个验收 SKU 图像的图像项成本约为 $0.048。", "1K Nano Banana 2에서 image-output leg 1,000개는 일반 B2C 기준 50% 할인 후 bounded input을 더해 $33.60에 정산됩니다. settled 총액을 accepted asset 수로 나누면 실제 수치가 나오며 acceptance rate 70%면 accepted SKU image당 image-leg 비용은 약 $0.048입니다."),
      ),
      faq(
        tr("Does the 50% discount cover CDN and review costs?", "Покрывает ли скидка 50% расходы на CDN и review?", "五折是否涵盖 CDN 与审核成本？", "50% 할인이 CDN과 review 비용을 포함하나요?"),
        tr("No. It applies to the exact official model usage for regular B2C accounts — generation and edit requests. Storage, WebP/AVIF transformation, CDN delivery, human review and rejected assets are separate application costs that stay in your business case.", "Нет. Она применяется к exact official model usage обычных B2C-аккаунтов — запросам на генерацию и edits. Storage, WebP/AVIF transformation, CDN delivery, human review и rejected assets — отдельные application costs, которые остаются в вашем business case.", "不涵盖。它适用于普通 B2C 账户的准确官方模型 usage——生成与编辑请求。存储、WebP/AVIF 转换、CDN 分发、人工审核与被拒资产是独立的应用成本，仍计入你的商业模型。", "아닙니다. 일반 B2C 계정의 exact official model usage(생성·edit 요청)에 적용됩니다. storage, WebP/AVIF transformation, CDN delivery, human review, rejected asset은 business case에 남는 별도 application cost입니다."),
      ),
      faq(
        tr("Is there a free way to test before funding a catalog run?", "Есть ли бесплатный способ теста до финансирования прогона каталога?", "在为目录批次充值前有免费测试方式吗？", "카탈로그 실행에 충전하기 전 묣료 테스트 방법이 있나요?"),
        tr("Yes. New B2C accounts created with Google or GitHub get $5 of platform bonus credit, valid on Claude, GPT and Gemini models. For Nano Banana 2 you can also call countTokens to estimate input for free before any image is billed. When you top up, bank card and cryptocurrency are both accepted.", "Да. Новые B2C-аккаунты через Google или GitHub получают бонус $5 на баланс платформы — он действует для моделей Claude, GPT и Gemini. Для Nano Banana 2 можно бесплатно вызвать countTokens и оценить input до того, как будет оплачено хоть одно изображение. При пополнении принимаются банковская карта и криптовалюта.", "有。通过 Google 或 GitHub 创建的新 B2C 账户可获得 $5 平台奖励金，适用于 Claude、GPT 和 Gemini 模型。对 Nano Banana 2，还可以在任何图像计费之前免费调用 countTokens 估算输入。充值时支持银行卡与加密货币。", "네. Google이나 GitHub로 만든 신규 B2C 계정은 Claude, GPT, Gemini 모델에 유효한 $5 플랫폼 본너스 크레딧을 받습니다. Nano Banana 2는 이미지가 과금되기 전 countTokens를 물료로 호출해 input을 추정할 수도 있습니다. 충전 시 은행 카드와 암호화폐 모두 지원됩니다."),
      ),
    ],
  };
