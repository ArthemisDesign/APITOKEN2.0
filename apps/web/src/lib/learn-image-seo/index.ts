// Image-route SEO specs, one module per spec. Aggregated here in the original
// order from learn-image-seo.ts and projected into the four locales.

import type { LearnArticle, Locale, LocalizedContent } from "../learn";
import type { ImageSeoSpec } from "./shared";
import { spec as nanoBanana2ApiCost } from "./nano-banana-2-api-cost";
import { spec as gptImage2ApiCost } from "./gpt-image-2-api-cost";
import { spec as nanoBanana2VsGptImage2 } from "./nano-banana-2-vs-gpt-image-2";
import { spec as imageGenerationApiPricing } from "./image-generation-api-pricing";
import { spec as cheapestImageGenerationApi } from "./cheapest-image-generation-api";
import { spec as imageEditingApiGuide } from "./image-editing-api-guide";
import { spec as batchImageGenerationApi } from "./batch-image-generation-api";
import { spec as imageGenerationApiForEcommerce } from "./image-generation-api-for-ecommerce";

const imageSeoSpecs: ImageSeoSpec[] = [
  nanoBanana2ApiCost,
  gptImage2ApiCost,
  nanoBanana2VsGptImage2,
  imageGenerationApiPricing,
  cheapestImageGenerationApi,
  imageEditingApiGuide,
  batchImageGenerationApi,
  imageGenerationApiForEcommerce,
];

function contentFor(spec: ImageSeoSpec, locale: Locale): LocalizedContent {
  return {
    title: spec.title[locale],
    h1: spec.h1[locale],
    description: spec.description[locale],
    keywords: spec.keywords[locale],
    dek: spec.dek[locale],
    sections: spec.sections.map((item) => item[locale]),
    faq: spec.faq.map((item) => item[locale]),
  };
}

export const learnImageSeoEn: LearnArticle[] = imageSeoSpecs.map((spec) => ({
  slug: spec.slug,
  cluster: spec.cluster,
  related: spec.related,
  ...contentFor(spec, "en"),
  published: "2026-08-09",
  updated: "2026-08-09",
}));

export const learnImageSeoRu: Record<string, LocalizedContent> = Object.fromEntries(
  imageSeoSpecs.map((spec) => [spec.slug, contentFor(spec, "ru")]),
);

export const learnImageSeoZh: Record<string, LocalizedContent> = Object.fromEntries(
  imageSeoSpecs.map((spec) => [spec.slug, contentFor(spec, "zh")]),
);

export const learnImageSeoKo: Record<string, LocalizedContent> = Object.fromEntries(
  imageSeoSpecs.map((spec) => [spec.slug, contentFor(spec, "ko")]),
);
