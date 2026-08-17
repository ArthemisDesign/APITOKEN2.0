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
      read("lib/learn-provider-en.ts"),
      read("lib/learn-provider-ru.ts"),
      read("lib/learn-provider-ko.ts"),
      read("lib/learn-provider-zh.ts"),
      read("lib/learn-image-seo.ts"),
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
});
