import type { ImageSeoSpec } from "./shared";
import { faq, list, note, paragraph, section, steps, table, tr } from "./shared";

export const spec: ImageSeoSpec = {
    slug: "nano-banana-2-vs-gpt-image-2",
    cluster: "compare",
    related: ["nano-banana-2-api-cost", "gpt-image-2-api-cost", "cheapest-image-generation-api", "image-editing-api-guide"],
    title: tr(
      "Nano Banana 2 vs GPT Image 2 API: Cost and Capabilities",
      "Nano Banana 2 vs GPT Image 2 API: цена и возможности",
      "Nano Banana 2 vs GPT Image 2 API：成本与能力",
      "Nano Banana 2 vs GPT Image 2 API: 비용과 기능",
    ),
    h1: tr(
      "Nano Banana 2 vs GPT Image 2 for image generation",
      "Nano Banana 2 или GPT Image 2 для генерации изображений",
      "Nano Banana 2 与 GPT Image 2 图像生成对比",
      "이미지 생성용 Nano Banana 2와 GPT Image 2 비교",
    ),
    description: tr(
      "Compare Nano Banana 2 and GPT Image 2 by price, protocol, image sizes, reference limits, output format and the workloads where each image API is the safer choice.",
      "Сравните Nano Banana 2 и GPT Image 2 по цене, protocol, размерам, references, формату output и сценариям, где каждый image API подходит лучше.",
      "按价格、协议、图像尺寸、参考图限制、输出格式与适用工作负载比较 Nano Banana 2 和 GPT Image 2。",
      "가격, protocol, 이미지 크기, reference 제한, output 형식과 workload별로 Nano Banana 2와 GPT Image 2를 비교합니다.",
    ),
    keywords: tr(
      ["nano banana 2 vs gpt image 2", "best image generation api", "gpt image vs gemini image", "nano banana api comparison", "ai image api comparison", "image generation api cost"],
      ["nano banana 2 или gpt image 2", "лучший api генерации изображений", "gpt image vs gemini image", "сравнение nano banana api", "сравнение ai image api", "стоимость image generation api"],
      ["nano banana 2 对比 gpt image 2", "最佳图像生成 api", "gpt image 对比 gemini image", "nano banana api 对比", "ai 图像 api 对比", "图像生成 api 成本"],
      ["nano banana 2 vs gpt image 2", "최고의 이미지 생성 api", "gpt image vs gemini image", "nano banana api 비교", "ai image api 비교", "이미지 생성 api 비용"],
    ),
    dek: tr(
      "Nano Banana 2 offers explicit 1K/2K/4K sizes, broad aspect ratios and up to 14 references on the native Gemini shape. GPT Image 2 offers a narrow OpenAI Images route with one-to-five PNG references and terminal token billing. Both receive the regular B2C 50% discount.",
      "Nano Banana 2 даёт явные 1K/2K/4K, широкий набор aspect ratios и до 14 references в native Gemini shape. GPT Image 2 использует узкий OpenAI Images route с 1–5 PNG и terminal token billing. На обе модели действует обычная B2C-скидка 50%.",
      "Nano Banana 2 在原生 Gemini 结构中提供明确的 1K/2K/4K、丰富宽高比与最多 14 张参考图。GPT Image 2 使用有界 OpenAI Images 路由，支持 1–5 张 PNG，并按终态 token 计费。普通 B2C 两者均五折。",
      "Nano Banana 2는 native Gemini 형식에서 명시적 1K/2K/4K, 다양한 aspect ratio, 최대 14 references를 제공합니다. GPT Image 2는 1~5 PNG와 terminal token billing의 제한된 OpenAI Images route를 사용하며 둘 다 일반 B2C 50% 할인을 받습니다.",
    ),
    sections: [
      section(
        tr("Side-by-side contract", "Контракт side by side", "并排比较接口契约", "계약 나란히 비교"),
        [
          table(
            { headers: ["Decision", "Nano Banana 2", "GPT Image 2"], rows: [["Model", "gemini-3.1-flash-image", "gpt-image-2"], ["Protocol", "Gemini generateContent + x-goog-api-key", "OpenAI Images + Bearer"], ["Output", "inlineData image part", "one non-streaming base64 PNG"], ["Published sizes", "1K, 2K, 4K", "auto; exact dimensions not promised"], ["References", "up to 14 supported image inputs", "1–5 strict PNG files"], ["Price authority", "fixed image tokens by size + other legs", "terminal text/image input and image-output usage"]] },
            { headers: ["Критерий", "Nano Banana 2", "GPT Image 2"], rows: [["Модель", "gemini-3.1-flash-image", "gpt-image-2"], ["Protocol", "Gemini generateContent + x-goog-api-key", "OpenAI Images + Bearer"], ["Output", "inlineData image part", "один non-streaming base64 PNG"], ["Размеры", "1K, 2K, 4K", "auto; exact dimensions не обещаны"], ["References", "до 14 image inputs", "1–5 строгих PNG"], ["Price authority", "фиксированные image tokens по size + другие legs", "terminal text/image input и image-output usage"]] },
            { headers: ["决策项", "Nano Banana 2", "GPT Image 2"], rows: [["模型", "gemini-3.1-flash-image", "gpt-image-2"], ["协议", "Gemini generateContent + x-goog-api-key", "OpenAI Images + Bearer"], ["输出", "inlineData 图像 part", "单张非流式 base64 PNG"], ["已发布尺寸", "1K、2K、4K", "auto；不承诺准确尺寸"], ["参考图", "最多 14 张受支持图像输入", "1–5 张严格 PNG"], ["价格权威", "按尺寸固定 image token + 其他项", "终态 text/image input 与 image-output usage"]] },
            { headers: ["결정", "Nano Banana 2", "GPT Image 2"], rows: [["모델", "gemini-3.1-flash-image", "gpt-image-2"], ["Protocol", "Gemini generateContent + x-goog-api-key", "OpenAI Images + Bearer"], ["Output", "inlineData image part", "non-streaming base64 PNG 한 장"], ["공개 크기", "1K, 2K, 4K", "auto; 정확한 dimensions 미보장"], ["References", "최대 14개 지원 image input", "1~5 strict PNG"], ["Price authority", "크기별 고정 image token + 기타 leg", "terminal text/image input 및 image-output usage"]] },
          ),
          note(
            "The protocols are not interchangeable. A successful model choice still fails if a Gemini inlineData response is sent to a client that only knows the OpenAI Images schema.",
            "Protocols невзаимозаменяемы. Даже правильная модель не поможет, если Gemini inlineData попадёт в client, понимающий только OpenAI Images schema.",
            "两种协议不可互换。如果把 Gemini inlineData 响应交给只理解 OpenAI Images schema 的客户端，正确的模型选择仍会失败。",
            "protocol은 교환할 수 없습니다. Gemini inlineData 응답을 OpenAI Images schema만 아는 client에 보내면 올바른 모델 선택도 실패합니다.",
          ),
        ],
      ),
      section(
        tr("Choose by acceptance criteria", "Выбирайте по acceptance criteria", "按验收标准选择", "acceptance 기준으로 선택"),
        [
          steps(
            ["Write a fixed test set with text-only generation, one-reference editing and your hardest aspect ratio.", "Run the same visual brief on both models without silently changing resolution or references.", "Score instruction fidelity, product/reference fidelity, artifacts, latency and settled charge.", "Pin the winner per asset class; do not make one image model the global default without evidence."],
            ["Соберите fixed test set: text-only generation, edit с одной reference и самый сложный aspect ratio.", "Запустите один visual brief на обеих моделях без скрытой смены resolution или references.", "Оцените instruction fidelity, product/reference fidelity, artifacts, latency и settled charge.", "Закрепите победителя по классу ассетов, а не делайте одну image-модель global default без evidence."],
            ["建立固定测试集：纯文本生成、单参考图编辑以及最困难的宽高比。", "两种模型使用同一视觉 brief，不要静默改变分辨率或参考图。", "评分指令遵循、产品/参考图一致性、瑕疵、延迟与结算费用。", "按资产类别固定胜出模型，不要在缺乏证据时设为全局默认。"],
            ["text-only generation, reference 한 장 edit, 가장 어려운 aspect ratio의 고정 test set을 만듭니다.", "resolution이나 references를 몰래 바꾸지 않고 두 모델에 같은 visual brief를 실행합니다.", "instruction fidelity, product/reference fidelity, artifact, latency, settled charge를 채점합니다.", "근거 없이 한 image model을 global default로 두지 말고 asset class별 승자를 고정합니다."],
          ),
          paragraph(
            "Choose Nano Banana 2 when explicit size/aspect controls or many references are contract requirements. Choose GPT Image 2 when an existing OpenAI Images client, strict PNG edit flow or one-output contract reduces integration risk. Quality still needs your own eval.",
            "Выбирайте Nano Banana 2, если нужны явные size/aspect controls или много references. GPT Image 2 подходит, когда существующий OpenAI Images client, строгий PNG edit flow или контракт одного output снижает integration risk. Качество всё равно требует вашего eval.",
            "明确尺寸/宽高比或多参考图是硬性要求时选 Nano Banana 2；已有 OpenAI Images 客户端、严格 PNG 编辑流程或单输出契约能降低集成风险时选 GPT Image 2。质量仍需自有评测。",
            "명시적 size/aspect control이나 많은 references가 계약 요건이면 Nano Banana 2, 기존 OpenAI Images client·strict PNG edit·one-output 계약이 통합 위험을 줄이면 GPT Image 2를 선택하세요. 품질은 자체 eval이 필요합니다.",
          ),
        ],
      ),
      section(
        tr("Compare total cost, not one headline", "Сравнивайте total cost, а не один headline", "比较总成本而非单一报价", "한 가지 headline이 아닌 총비용 비교"),
        [
          list(
            ["Nano Banana 2 has a predictable image-output leg by size, but input, text/thinking and grounding can add cost.", "GPT Image 2 combines actual token legs; reference edits cost more input than prompt-only generation.", "Both are 50% off official spend for regular B2C, so retries and acceptance rate often decide the cheaper workflow.", "Compare the charge per accepted asset, including failed outputs, storage, validation and human review."],
            ["У Nano Banana 2 предсказуемый image-output leg по size, но input, text/thinking и grounding добавляют стоимость.", "GPT Image 2 складывает actual token legs; reference edits требуют больше input, чем prompt-only generation.", "Обе модели стоят на 50% дешевле official spend для обычного B2C, поэтому итог часто решают retries и acceptance rate.", "Сравнивайте charge на принятый ассет, включая неудачные outputs, storage, validation и human review."],
            ["Nano Banana 2 的图像输出项可按尺寸预测，但输入、文本/思考与 grounding 会增加成本。", "GPT Image 2 汇总实际 token 项；参考图编辑的输入成本高于纯提示词生成。", "普通 B2C 两者均为官方成本五折，因此重试与验收率常决定哪个工作流更便宜。", "比较每个已验收资产的费用，并计入失败输出、存储、验证与人工审核。"],
            ["Nano Banana 2는 크기별 image-output leg가 예측 가능하지만 input, text/thinking, grounding 비용이 더해집니다.", "GPT Image 2는 actual token leg를 합산하며 reference edit은 prompt-only generation보다 input 비용이 큽니다.", "일반 B2C에서 둘 다 official spend의 50%이므로 retry와 acceptance rate가 더 저렴한 workflow를 결정하는 경우가 많습니다.", "실패 output, storage, validation, human review를 포함한 accepted asset당 charge를 비교합니다."],
          ),
        ],
      ),
    ],
    faq: [
      faq(tr("Which API is cheaper, Nano Banana 2 or GPT Image 2?", "Какой API дешевле: Nano Banana 2 или GPT Image 2?", "Nano Banana 2 和 GPT Image 2 哪个更便宜？", "Nano Banana 2와 GPT Image 2 중 어느 API가 더 저렴한가요?"), tr("There is no universal winner. Nano Banana 2 exposes a fixed image-output cost by size; GPT Image 2 settles variable token legs. Compare cost per accepted asset on your prompts and references.", "Универсального победителя нет. Nano Banana 2 даёт fixed image-output cost по size, GPT Image 2 — variable token legs. Сравните cost per accepted asset на своих prompts и references.", "没有通用答案。Nano Banana 2 按尺寸提供固定图像输出成本；GPT Image 2 结算可变 token 项。应使用自己的提示词与参考图比较每个验收资产的成本。", "보편적 승자는 없습니다. Nano Banana 2는 크기별 고정 image-output 비용, GPT Image 2는 가변 token leg를 정산하므로 실제 prompt/reference의 accepted asset당 비용을 비교하세요.")),
      faq(tr("Which model supports more reference images?", "Какая модель поддерживает больше references?", "哪款模型支持更多参考图？", "어느 모델이 더 많은 reference image를 지원하나요?"), tr("Nano Banana 2 accepts up to 14 supported image inputs. GPT Image 2 edits accept one to five strict PNG references on the published route.", "Nano Banana 2 принимает до 14 поддерживаемых image inputs. Публичный GPT Image 2 edits route принимает 1–5 строгих PNG references.", "Nano Banana 2 最多接受 14 张受支持图像输入；GPT Image 2 已发布编辑路由接受 1–5 张严格 PNG 参考图。", "Nano Banana 2는 최대 14개 지원 image input, GPT Image 2 공개 edit route는 1~5 strict PNG references를 받습니다.")),
      faq(tr("Can both models use the same API request?", "Можно ли вызвать обе модели одним API request?", "两款模型能使用同一种 API 请求吗？", "두 모델에 같은 API request를 쓸 수 있나요?"), tr("No. Nano Banana 2 uses native Gemini generateContent and x-goog-api-key; GPT Image 2 uses OpenAI Images routes and Authorization: Bearer.", "Нет. Nano Banana 2 использует native Gemini generateContent и x-goog-api-key; GPT Image 2 — OpenAI Images routes и Authorization: Bearer.", "不能。Nano Banana 2 使用原生 Gemini generateContent 与 x-goog-api-key；GPT Image 2 使用 OpenAI Images 路由与 Authorization: Bearer。", "아닙니다. Nano Banana 2는 native Gemini generateContent와 x-goog-api-key, GPT Image 2는 OpenAI Images route와 Authorization: Bearer를 사용합니다.")),
      faq(tr("Do both receive the 50% discount?", "На обе модели действует скидка 50%?", "两款模型都享受五折吗？", "두 모델 모두 50% 할인을 받나요?"), tr("Yes for regular B2C accounts after exact official usage is calculated. B2B and OpenKeys use their own pricing policies.", "Да, для обычных B2C после расчёта exact official usage. У B2B и OpenKeys свои pricing policies.", "普通 B2C 在准确官方 usage 计算后，两款模型都享受五折；B2B 与 OpenKeys 使用各自定价策略。", "일반 B2C는 exact official usage 계산 후 둘 다 50% 할인을 받으며 B2B/OpenKeys는 자체 pricing policy를 사용합니다.")),
    ],
  };
