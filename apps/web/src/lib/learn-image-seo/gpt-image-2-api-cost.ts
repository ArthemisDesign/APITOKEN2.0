import type { ImageSeoSpec } from "./shared";
import { faq, list, note, paragraph, section, sharedCode, steps, table, tr, OPENAI } from "./shared";

export const spec: ImageSeoSpec = {
    slug: "gpt-image-2-api-cost",
    cluster: "explain",
    related: ["gpt-image-2-api-guide", "nano-banana-2-api-cost", "image-generation-api-pricing", "image-editing-api-guide"],
    title: tr(
      "GPT Image 2 API Cost: Save 50% on Generation and Edits",
      "Стоимость GPT Image 2 API: экономия 50% на генерации и edits",
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
      "Calculate GPT Image 2 generation and edit cost from text input, image input, cached input and image-output tokens, with apiToken.sale's flat 50% B2C discount.",
      "Рассчитайте стоимость генерации и edits GPT Image 2 по text input, image input, cached input и image-output tokens со скидкой apiToken.sale 50% для B2C.",
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
      "GPT Image 2 does not have one honest fixed price per picture: the settled total follows terminal text-input, image-input, cached-input and image-output usage. Regular B2C pays exactly half of the resulting official cost.",
      "У GPT Image 2 нет одной честной фиксированной цены картинки: итог определяется terminal usage для text input, image input, cached input и image output. Обычный B2C платит ровно половину рассчитанной официальной стоимости.",
      "GPT Image 2 没有诚实的单张固定价格：结算总额由终态 text input、image input、cached input 与 image output usage 决定。普通 B2C 支付计算后官方成本的一半。",
      "GPT Image 2에는 정직한 이미지당 고정 가격이 없습니다. terminal text input, image input, cached input, image output usage로 정산하며 일반 B2C는 공식 비용의 정확히 절반을 냅니다.",
    ),
    sections: [
      section(
        tr("Every billed leg", "Все составляющие цены", "全部计费项", "모든 과금 leg"),
        [
          table(
            { headers: ["Usage leg", "Official per 1M", "Regular B2C here"], rows: [["Fresh text input", "$5", "$2.50"], ["Fresh image input", "$8", "$4"], ["Cached text input", "$1.25", "$0.625"], ["Cached image input", "$2", "$1"], ["Image output", "$30", "$15"]] },
            { headers: ["Usage leg", "Официально за 1M", "Обычный B2C здесь"], rows: [["Fresh text input", "$5", "$2.50"], ["Fresh image input", "$8", "$4"], ["Cached text input", "$1.25", "$0.625"], ["Cached image input", "$2", "$1"], ["Image output", "$30", "$15"]] },
            { headers: ["Usage 项", "官方每 1M", "本站普通 B2C"], rows: [["Fresh text input", "$5", "$2.50"], ["Fresh image input", "$8", "$4"], ["Cached text input", "$1.25", "$0.625"], ["Cached image input", "$2", "$1"], ["Image output", "$30", "$15"]] },
            { headers: ["Usage leg", "공식 1M당", "일반 B2C 가격"], rows: [["Fresh text input", "$5", "$2.50"], ["Fresh image input", "$8", "$4"], ["Cached text input", "$1.25", "$0.625"], ["Cached image input", "$2", "$1"], ["Image output", "$30", "$15"]] },
          ),
          paragraph(
            "The formula is sum(tokens in each leg × its official rate) × 0.5 for regular B2C. Cached text and image input are 25% of their fresh official rates before the B2C discount is applied.",
            "Формула для обычного B2C: сумма(tokens каждого leg × official rate) × 0.5. Cached text и image input сначала считаются как 25% от fresh official rate, затем применяется B2C-скидка.",
            "普通 B2C 公式为：各项 token × 官方费率之和，再乘 0.5。cached text/image input 先按 fresh 官方费率的 25% 计算，再应用 B2C 折扣。",
            "일반 B2C 공식은 각 leg token × 공식 요금의 합 × 0.5입니다. cached text/image input은 fresh 공식 요금의 25%를 계산한 뒤 B2C 할인을 적용합니다.",
          ),
        ],
      ),
      section(
        tr("Measure a real request", "Измерьте реальный запрос", "测量真实请求", "실제 요청 측정"),
        [
          steps(
            ["Use gpt-image-2 on POST /v1/images/generations for a new asset or /v1/images/edits for one to five strict PNG references.", "Keep the shipped profile bounded: one PNG output, non-streaming, with only the documented opaque/low/auto controls.", "Read terminal usage rather than estimating output tokens from PNG bytes or dimensions.", "Match the request with the dashboard charge and keep its ID beside the generated asset."],
            ["Для нового ассета вызовите gpt-image-2 через POST /v1/images/generations, для edits с 1–5 строгими PNG — /v1/images/edits.", "Сохраняйте подтверждённый профиль: один PNG без стриминга и только документированные controls opaque/low/auto.", "Читайте terminal usage, не оценивайте output tokens по размеру PNG или dimensions.", "Сверьте request с charge в дашборде и храните его ID рядом с ассетом."],
            ["新资产使用 gpt-image-2 调用 POST /v1/images/generations；1–5 张严格 PNG 参考图编辑使用 /v1/images/edits。", "保持已发布的有界配置：单张非流式 PNG，仅使用已记录的 opaque/low/auto 控制。", "读取终态 usage，不要按 PNG 字节或尺寸估算输出 token。", "将请求与仪表板扣费核对，并把 request ID 与生成资产一起保存。"],
            ["새 asset은 gpt-image-2의 POST /v1/images/generations, 1~5 strict PNG edit은 /v1/images/edits를 사용합니다.", "한 장의 non-streaming PNG와 문서화된 opaque/low/auto control만 쓰는 bounded profile을 유지합니다.", "PNG bytes나 dimensions로 output token을 추정하지 말고 terminal usage를 읽습니다.", "request를 dashboard charge와 대조하고 ID를 생성 asset과 함께 보관합니다."],
          ),
          sharedCode(`curl ${OPENAI}/images/generations \\
  -H "Authorization: Bearer $APITOKEN_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{"model":"gpt-image-2","prompt":"A clean studio product photograph","background":"opaque","quality":"low","size":"auto"}'`),
        ],
      ),
      section(
        tr("Savings that are actually measurable", "Экономия, которую можно измерить", "真正可测量的节省", "실제로 측정 가능한 절감"),
        [
          list(
            ["Use generation for a new asset and edits only when references improve acceptance; references add image-input cost.", "Reuse provider-recognized cached inputs where terminal usage proves the cached leg instead of assuming every repeated file is cached.", "Reject unusable outputs with a fixed visual checklist; endless prompt retries erase a nominal token discount.", "Give the image worker its own key and lifetime spending limit so batch volume cannot consume the entire account balance."],
            ["Для нового ассета используйте generation, а edits — только когда references повышают acceptance: они добавляют image-input cost.", "Считайте input cached только когда это подтверждает terminal usage; повторный файл не гарантирует cache.", "Проверяйте результат фиксированным visual checklist: бесконечные prompt retries съедают скидку.", "Выдайте image-worker отдельный ключ с общим лимитом расходов, чтобы batch не потратил весь баланс аккаунта."],
            ["新资产使用 generation；仅当参考图能提升验收率时使用 edits，因为参考图会增加 image-input 成本。", "只有终态 usage 明确证明 cached 项时才计入缓存，不要假设重复文件必然缓存。", "用固定视觉清单拒绝不可用输出；无尽提示词重试会抹掉名义折扣。", "为图像 worker 使用独立密钥与终身消费上限，避免 batch 耗尽整个账户余额。"],
            ["새 asset은 generation을 쓰고 reference가 acceptance를 높일 때만 edit을 사용합니다. reference는 image-input 비용을 더합니다.", "반복 파일이 항상 cache된다고 가정하지 말고 terminal usage가 cached leg를 증명할 때만 인정합니다.", "고정 visual checklist로 실패 output을 거부해 무한 prompt retry가 할인을 없애지 않게 합니다.", "image worker 전용 key와 평생 누적 지출 한도로 batch가 전체 잔액을 소진하지 않게 합니다."],
          ),
          note(
            "Do not publish a made-up price per image. GPT Image 2 output usage varies, exact dimensions are not promised on this subscription wire, and the terminal usage event is the billing authority.",
            "Не публикуйте выдуманную цену картинки. У GPT Image 2 меняется output usage, этот subscription wire не обещает точные dimensions, а billing authority — terminal usage event.",
            "不要发布虚构的单张价格。GPT Image 2 的 output usage 会变化，该订阅传输不承诺准确尺寸，结算权威是终态 usage event。",
            "가짜 이미지당 가격을 게시하지 마세요. GPT Image 2 output usage는 변하고 이 subscription wire는 정확한 dimensions를 약속하지 않으며 billing authority는 terminal usage event입니다.",
          ),
        ],
      ),
    ],
    faq: [
      faq(
        tr("How much does GPT Image 2 cost here?", "Сколько стоит GPT Image 2 здесь?", "本站 GPT Image 2 多少钱？", "여기서 GPT Image 2 비용은 얼마인가요?"),
        tr("A regular B2C account pays $2.50/M fresh text input, $4/M fresh image input and $15/M image output, with cached input at one quarter of fresh before the same 50% discount. The request total follows terminal usage.", "Обычный B2C платит $2.50/M fresh text input, $4/M fresh image input и $15/M image output; cached input сначала равен четверти fresh, затем получает ту же скидку 50%. Итог берётся из terminal usage.", "普通 B2C 的 fresh text input 为 $2.50/M、fresh image input 为 $4/M、image output 为 $15/M；cached input 先按 fresh 的四分之一计算，再享受相同五折。总额以终态 usage 为准。", "일반 B2C는 fresh text input $2.50/M, fresh image input $4/M, image output $15/M이며 cached input은 fresh의 1/4 계산 후 동일한 50% 할인을 받습니다. 합계는 terminal usage를 따릅니다."),
      ),
      faq(
        tr("Why is there no fixed price for one GPT Image 2 picture?", "Почему нет фиксированной цены одной картинки GPT Image 2?", "为什么没有 GPT Image 2 单张固定价格？", "왜 GPT Image 2 이미지당 고정 가격이 없나요?"),
        tr("Because billing combines actual text input, optional image references, cached usage and the image-output tokens reported for that request. PNG byte size is not the billing formula.", "Потому что billing складывает фактический text input, references, cached usage и image-output tokens конкретного запроса. Размер PNG в байтах не является формулой цены.", "因为结算组合了实际 text input、可选参考图、cached usage 与该请求报告的 image-output token；PNG 字节大小不是计费公式。", "실제 text input, 선택적 image reference, cached usage, 해당 요청의 image-output token을 합산하기 때문입니다. PNG byte 크기는 과금 공식이 아닙니다."),
      ),
      faq(
        tr("Are edits included in the 50% discount?", "На edits тоже действует скидка 50%?", "编辑也享受五折吗？", "edit에도 50% 할인이 적용되나요?"),
        tr("Yes for regular B2C: the policy applies to the complete official cost, including image-input references and image output. B2B and OpenKeys follow their own account-class rules.", "Да, для обычного B2C: политика применяется ко всей official cost, включая image-input references и image output. У B2B и OpenKeys свои правила класса аккаунта.", "普通 B2C 是：策略应用于完整官方成本，包括参考图 image input 与 image output。B2B 和 OpenKeys 遵循各自账户类型规则。", "일반 B2C에는 적용됩니다. reference image input과 image output을 포함한 전체 official cost에 정책이 적용되며 B2B/OpenKeys는 자체 account-class 규칙을 따릅니다."),
      ),
      faq(
        tr("Does GPT Image 2 support transparent backgrounds here?", "Поддерживает ли GPT Image 2 прозрачный фон здесь?", "本站 GPT Image 2 支持透明背景吗？", "여기 GPT Image 2는 투명 배경을 지원하나요?"),
        tr("No. The proved public profile supports opaque/low/auto and one PNG output; transparent background is rejected rather than silently approximated.", "Нет. Подтверждённый public profile поддерживает opaque/low/auto и один PNG; transparent background отклоняется, а не имитируется.", "不支持。已验证公开配置支持 opaque/low/auto 与单张 PNG；透明背景会被拒绝，不会静默近似。", "아닙니다. 검증된 public profile은 opaque/low/auto와 PNG 한 장을 지원하며 투명 배경은 조용히 근사하지 않고 거부합니다."),
      ),
    ],
  };
