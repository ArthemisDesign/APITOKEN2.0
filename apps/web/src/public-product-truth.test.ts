import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const sourceRoot = join(process.cwd(), "src");
const read = (path: string) => readFileSync(join(sourceRoot, path), "utf8");

describe("public product truth regressions", () => {
  it("does not advertise unimplemented key-control features", () => {
    const copy = [
      read("app/page.tsx"),
      read("app/changelog/page.tsx"),
      read("lib/messages.json"),
      read("lib/learn.ts"),
      read("lib/learn-ru.ts"),
      read("lib/learn-ko.ts"),
      read("lib/learn-zh.ts"),
      read("lib/learn-og.tsx"),
    ].join("\n");

    expect(copy).not.toMatch(/daily and monthly spend caps|request caps are configurable|model scoping|IP controls|rotation without downtime|scope keys to tools/i);
    expect(copy).not.toMatch(/дневн.*месячн.*лимит|контроль по IP|ротаци[^\n]*без простоя/i);
    expect(copy).not.toMatch(/일일.*월별.*지출|IP 제어|다운타임 없이/i);
    expect(copy).not.toMatch(/每日.*每月.*消费|IP 管控|不停机轮换/i);
    expect(copy).not.toMatch(/99\.9%|<100ms|API3 key/i);
    expect(copy).toContain("lifetime spending limit");
    expect(copy).toContain("expiration date");
  });

  it("keeps empty editorial navigation useful and renders mobile model pricing", () => {
    const header = read("components/site-chrome.tsx");
    const blog = read("app/blog/page.tsx");
    const model = read("app/models/[slug]/page.tsx");
    const styles = read("app/globals.css");

    expect(header).not.toContain('<Link href="/blog">Blog</Link>');
    expect(header).toContain('k="nav_guides"');
    expect(blog).toContain("No field notes published yet");
    expect(blog).toContain('href="/docs/learn"');
    expect(model).toContain('className="model-pricing-mobile"');
    expect(styles).toContain(".model-pricing-mobile{display:grid");
  });

  it("keeps the retired progressive discount out of current public copy", () => {
    const copy = [
      read("lib/messages.json"),
      read("lib/seo-core.ts"),
      read("lib/learn.ts"),
      read("lib/learn-ru.ts"),
      read("lib/learn-ko.ts"),
      read("lib/learn-zh.ts"),
      read("lib/learn-og.tsx"),
    ].join("\n");

    expect(copy).not.toMatch(/progressive (?:discount|pricing)|up to 70%|60\s*[–—-]\s*70%/i);
    expect(copy).not.toMatch(/прогрессивн(?:ая|ые|ой) (?:скидк|цен)|до 70%|60\s*[–—-]\s*70%/i);
    expect(copy).toMatch(/flat 50% discount/i);
    expect(copy).toMatch(/плоск(?:ая|ой) скидк(?:а|ой) 50%/i);
  });
});
