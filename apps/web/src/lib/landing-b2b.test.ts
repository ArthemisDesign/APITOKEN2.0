import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { describe, expect, it, vi } from "vitest";

const source = readFileSync(new URL("../../public/landing/b2b.js", import.meta.url), "utf8");

describe("static B2B email handoff", () => {
  for (const language of ["en", "ru"]) {
    it(`prepares an encoded ${language} draft without reporting network delivery`, () => {
      const handlers = new Map<string, (event: unknown) => void>();
      const form = { reportValidity: () => true, addEventListener: (name: string, handler: (event: unknown) => void) => handlers.set(name, handler) };
      const status = { hidden: true };
      const root = { lang: language, dataset: {} };
      const window = { location: { href: "" } };
      const fetch = vi.fn();
      const data = new Map([["company", "A & B\nTest"], ["email", "a+b@example.com"], ["spend", "20 000"]]);
      runInNewContext(source, {
        document: { documentElement: root, querySelector: (selector: string) => selector === "#b2bForm" ? form : selector === "#b2bOk" ? status : null, querySelectorAll: () => [] },
        window, fetch, FormData: class { get(key: string) { return data.get(key); } },
      });
      handlers.get("submit")!({ preventDefault() {}, currentTarget: form });
      const draft = new URL(window.location.href);
      expect(draft.protocol).toBe("mailto:");
      expect(draft.pathname).toBe("apitokensale@gmail.com");
      expect(draft.searchParams.get("body")).toContain("A & B\nTest");
      expect(draft.searchParams.get("body")).toContain("a+b@example.com");
      expect(fetch).not.toHaveBeenCalled();
      expect(status.hidden).toBe(false);
      const html = readFileSync(new URL(`../../public/landing/b2b${language === "en" ? "-en" : ""}.html`, import.meta.url), "utf8");
      expect(html).not.toContain("data-sent=");
      expect(html).toContain('role="status"');
    });
  }
});
