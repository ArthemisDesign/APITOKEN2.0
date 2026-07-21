import { describe, expect, it } from "vitest";
import { buildHubJsonLd } from "./learn-page";
import { absoluteUrl } from "./seo";

function breadcrumbTargets(locale: "en" | "ru" | "zh" | "ko"): string[] {
  const data = buildHubJsonLd(locale) as { "@graph": Array<{ itemListElement?: Array<{ item: string }> }> };
  return data["@graph"][0]?.itemListElement?.map((entry) => entry.item) ?? [];
}

describe("localized Learn breadcrumbs", () => {
  it("uses the real Russian home and docs routes", () => {
    expect(breadcrumbTargets("ru").slice(0, 2)).toEqual([absoluteUrl("/ru"), absoluteUrl("/ru/docs")]);
  });

  it("does not advertise nonexistent Chinese or Korean home/docs routes", () => {
    for (const locale of ["zh", "ko"] as const) {
      expect(breadcrumbTargets(locale).slice(0, 2), locale).toEqual([absoluteUrl("/"), absoluteUrl("/docs")]);
    }
  });
});
