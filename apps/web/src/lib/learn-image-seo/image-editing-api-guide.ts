import type { ImageSeoSpec } from "./shared";
import { faq, list, section, sharedCode, steps, table, tr, OPENAI } from "./shared";

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
      "Build an image-editing API workflow with GPT Image 2 or Nano Banana 2: reference limits, formats, endpoints, cost controls, validation and safe retry rules.",
      "Постройте image-editing API workflow на GPT Image 2 или Nano Banana 2: лимиты references, форматы, endpoints, контроль цены, validation и безопасные retries.",
      "使用 GPT Image 2 或 Nano Banana 2 构建图像编辑 API 工作流：参考图限制、格式、端点、成本控制、验证与安全重试。",
      "GPT Image 2 또는 Nano Banana 2로 reference 제한, 형식, endpoint, 비용 통제, validation, 안전한 retry를 갖춘 image-editing workflow를 만드세요.",
    ),
    keywords: tr(
      ["image editing api", "ai image edit api", "gpt image 2 edit api", "nano banana image editing", "reference image api", "product image editing api"],
      ["api редактирования изображений", "ai image edit api", "gpt image 2 edit api", "nano banana редактирование", "reference image api", "api обработки фото товара"],
      ["图像编辑 api", "ai 图像编辑 api", "gpt image 2 编辑 api", "nano banana 图像编辑", "参考图 api", "产品图像编辑 api"],
      ["이미지 편집 api", "ai image edit api", "gpt image 2 edit api", "nano banana 이미지 편집", "reference image api", "상품 이미지 편집 api"],
    ),
    dek: tr(
      "GPT Image 2 exposes a strict multipart PNG edit route. Nano Banana 2 treats references as native multimodal input and supports more formats and images. The right choice depends on the reference contract your application can validate.",
      "GPT Image 2 предоставляет strict multipart PNG edit route. Nano Banana 2 принимает references как native multimodal input и поддерживает больше форматов и изображений. Выбор зависит от reference contract, который приложение умеет валидировать.",
      "GPT Image 2 提供严格 multipart PNG 编辑路由。Nano Banana 2 把参考图作为原生多模态输入，支持更多格式与数量。应根据应用可验证的参考图契约选择。",
      "GPT Image 2는 strict multipart PNG edit route를, Nano Banana 2는 references를 native multimodal input으로 받아 더 많은 형식과 수량을 지원합니다. 앱이 검증할 수 있는 reference contract로 선택하세요.",
    ),
    sections: [
      section(
        tr("Choose the reference contract", "Выберите reference contract", "选择参考图契约", "reference contract 선택"),
        [
          table(
            { headers: ["Capability", "GPT Image 2", "Nano Banana 2"], rows: [["Route", "POST /v1/images/edits", "generateContent with inlineData"], ["References", "1–5", "up to 14"], ["Input files", "strict PNG", "PNG, JPEG, WEBP, HEIC, HEIF"], ["Output", "one base64 PNG", "image inlineData part"], ["Controls", "opaque / low / auto", "1K/2K/4K + published aspect ratios"]] },
            { headers: ["Capability", "GPT Image 2", "Nano Banana 2"], rows: [["Route", "POST /v1/images/edits", "generateContent с inlineData"], ["References", "1–5", "до 14"], ["Input files", "strict PNG", "PNG, JPEG, WEBP, HEIC, HEIF"], ["Output", "один base64 PNG", "image inlineData part"], ["Controls", "opaque / low / auto", "1K/2K/4K + aspect ratios"]] },
            { headers: ["能力", "GPT Image 2", "Nano Banana 2"], rows: [["路由", "POST /v1/images/edits", "含 inlineData 的 generateContent"], ["参考图", "1–5", "最多 14"], ["输入文件", "严格 PNG", "PNG、JPEG、WEBP、HEIC、HEIF"], ["输出", "单张 base64 PNG", "image inlineData part"], ["控制", "opaque / low / auto", "1K/2K/4K + 已发布宽高比"]] },
            { headers: ["기능", "GPT Image 2", "Nano Banana 2"], rows: [["Route", "POST /v1/images/edits", "inlineData 포함 generateContent"], ["References", "1~5", "최대 14"], ["Input files", "strict PNG", "PNG, JPEG, WEBP, HEIC, HEIF"], ["Output", "base64 PNG 한 장", "image inlineData part"], ["Controls", "opaque / low / auto", "1K/2K/4K + 공개 aspect ratio"]] },
          ),
          sharedCode(`curl ${OPENAI}/images/edits \\
  -H "Authorization: Bearer $APITOKEN_API_KEY" \\
  -F "model=gpt-image-2" \\
  -F "prompt=Keep the product unchanged and replace only the background" \\
  -F "image=@reference.png;type=image/png"`),
        ],
      ),
      section(
        tr("Validate before dispatch", "Валидируйте до dispatch", "发送前验证", "dispatch 전 검증"),
        [
          steps(
            ["Decode and inspect every reference server-side; reject unexpected MIME, empty files and oversized payloads before reservation.", "Write an edit brief that separates immutable product traits from the requested change.", "Choose the exact route and output contract your client can decode; do not mix inlineData with OpenAI Images parsing.", "After delivery, validate dimensions, format, product identity and prohibited changes before publishing.", "Store request ID and terminal usage with the source and result for rollback and cost attribution."],
            ["Декодируйте и проверяйте каждую reference server-side; отклоняйте unexpected MIME, empty и oversized payloads до reservation.", "Разделите в edit brief неизменяемые свойства продукта и requested change.", "Выберите route и output contract, который умеет декодировать client; не смешивайте inlineData с OpenAI Images parser.", "После delivery проверьте dimensions, format, product identity и запрещённые изменения.", "Храните request ID и terminal usage с source/result для rollback и cost attribution."],
            ["服务端解码并检查每张参考图；在预留前拒绝异常 MIME、空文件与超限 payload。", "在编辑 brief 中分离不可变产品特征与请求变更。", "选择客户端能解码的准确路由与输出契约，不要把 inlineData 与 OpenAI Images 解析混用。", "交付后验证尺寸、格式、产品一致性与禁止改动，再发布。", "把 request ID 与终态 usage 和源图/结果一起保存，以便回滚与成本归因。"],
            ["모든 reference를 server-side에서 decode·검사하고 reservation 전에 예상 밖 MIME, empty, oversized payload를 거부합니다.", "edit brief에서 불변 product 특성과 requested change를 분리합니다.", "client가 decode할 exact route/output contract를 고르고 inlineData와 OpenAI Images parsing을 섞지 않습니다.", "delivery 후 dimensions, format, product identity, 금지 변경을 검증한 뒤 게시합니다.", "rollback과 cost attribution을 위해 source/result에 request ID와 terminal usage를 저장합니다."],
          ),
        ],
      ),
      section(
        tr("Control edit cost and retries", "Контролируйте цену edits и retries", "控制编辑成本与重试", "edit 비용과 retry 통제"),
        [
          list(
            ["References are billable input. Send only files that constrain the requested edit.", "A provider-delivered edit is not safe to replay automatically after timeout ambiguity or output delivery.", "Limit variants and attempts per source asset; quality review must end the loop.", "Use separate keys for production edits and experiments so limits and attribution remain clear.", "Regular B2C gets 50% off official usage, but an unnecessary reference or retry is still unnecessary spend."],
            ["References — billable input. Отправляйте только файлы, ограничивающие requested edit.", "Provider-delivered edit нельзя автоматически replay после ambiguous timeout или output delivery.", "Ограничьте variants и attempts на source asset; quality review должен завершать loop.", "Разделите ключи production edits и experiments для ясных limits/attribution.", "Обычный B2C получает 50%, но ненужная reference или retry остаётся лишней тратой."],
            ["参考图属于计费输入，只发送确实约束编辑的文件。", "发生超时歧义或输出已交付后，不要自动重放提供商编辑。", "限制每个源资产的变体与尝试次数，让质量审核终止循环。", "生产编辑与实验使用不同密钥，保持限制与归因清晰。", "普通 B2C 可享五折，但不必要参考图或重试仍是浪费。"],
            ["reference는 billable input이므로 requested edit을 제약하는 파일만 보냅니다.", "ambiguous timeout이나 output delivery 후 provider-delivered edit을 자동 replay하지 않습니다.", "source asset당 variant와 attempt를 제한하고 quality review가 loop를 끝내게 합니다.", "production edit과 experiment key를 분리해 limit/attribution을 명확히 합니다.", "일반 B2C는 50% 할인되지만 불필요한 reference/retry는 여전히 낭비입니다."],
          ),
        ],
      ),
    ],
    faq: [
      faq(tr("Which API accepts more reference images?", "Какой API принимает больше references?", "哪个 API 接受更多参考图？", "어느 API가 더 많은 reference image를 받나요?"), tr("Nano Banana 2 accepts up to 14 supported image inputs. GPT Image 2 accepts one to five strict PNG files on its edits route.", "Nano Banana 2 принимает до 14 поддерживаемых image inputs. GPT Image 2 edits route принимает 1–5 строгих PNG.", "Nano Banana 2 最多接受 14 张受支持图像输入；GPT Image 2 编辑路由接受 1–5 张严格 PNG。", "Nano Banana 2는 최대 14개 지원 image input, GPT Image 2 edit route는 1~5 strict PNG를 받습니다.")),
      faq(tr("Can GPT Image 2 edit JPEG directly?", "Можно ли GPT Image 2 напрямую редактировать JPEG?", "GPT Image 2 能直接编辑 JPEG 吗？", "GPT Image 2가 JPEG를 직접 편집할 수 있나요?"), tr("Not on the published route. Convert and validate it as PNG before multipart upload, or use Nano Banana 2 when its supported JPEG input contract fits the workflow.", "Не на published route. Конвертируйте и проверьте JPEG как PNG до multipart upload либо используйте Nano Banana 2 с поддерживаемым JPEG contract.", "已发布路由不支持。multipart 上传前转换并验证为 PNG，或在适合工作流时使用支持 JPEG 输入的 Nano Banana 2。", "공개 route에서는 안 됩니다. multipart upload 전 PNG로 변환·검증하거나 지원 JPEG input contract가 맞으면 Nano Banana 2를 사용하세요.")),
      faq(tr("Do edits cost more than prompt-only generation?", "Edits дороже prompt-only generation?", "编辑是否比纯提示词生成更贵？", "edit이 prompt-only generation보다 비싼가요?"), tr("They add billable image input, so an otherwise comparable edit normally has more input cost. It may still be cheaper per accepted asset if references reduce retries.", "Они добавляют billable image input, поэтому comparable edit обычно дороже по input. Но он может быть дешевле per accepted asset, если references снижают retries.", "编辑增加计费 image input，因此可比编辑通常输入成本更高；若参考图减少重试，每个验收资产成本仍可能更低。", "billable image input이 추가되어 comparable edit은 보통 input 비용이 더 큽니다. reference가 retry를 줄이면 accepted asset당 비용은 더 낮을 수 있습니다.")),
      faq(tr("Can I retry an edit after a timeout?", "Можно ли retry edit после timeout?", "超时后能否重试编辑？", "timeout 후 edit을 retry해도 되나요?"), tr("Only when you can prove the prior attempt was not accepted. An ambiguous timeout may hide completed provider work; preserve the request ID and reconcile before another paid attempt.", "Только если доказано, что prior attempt не был принят. Ambiguous timeout может скрывать выполненную работу provider; сохраните request ID и проведите reconciliation до нового paid attempt.", "只有能证明前一次尝试未被接受时才可重试。歧义超时可能隐藏已完成的提供商工作；应保留 request ID 并先核对。", "prior attempt가 accepted되지 않았음을 증명할 때만 가능합니다. ambiguous timeout은 완료된 provider 작업을 숨길 수 있으므로 request ID를 보존하고 다음 paid attempt 전에 대조하세요.")),
    ],
  };
