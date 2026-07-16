import { articlesForLocale } from "@/lib/learn";
import { learnOgImage, OG_CONTENT_TYPE, OG_SIZE } from "@/lib/learn-og";

export const size = OG_SIZE;
export const contentType = OG_CONTENT_TYPE;

export function generateStaticParams() {
  return articlesForLocale("en").map((slug) => ({ slug }));
}

export default async function Image({ params }: { params: { slug: string } }) {
  return learnOgImage(params.slug, "en");
}
