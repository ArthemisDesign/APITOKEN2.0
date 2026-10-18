// @vitest-environment jsdom

import { readFileSync } from "node:fs";
import { URL as NodeURL } from "node:url";
import { runInNewContext } from "node:vm";
import { afterEach, describe, expect, it, vi } from "vitest";

type Estimate = { now: bigint; next: bigint; save: bigint; year: bigint };
type Calculator = {
  bind(options: {
    providers: unknown[];
    language: string;
    document: Document;
    onResult(result: Estimate): void;
    onInvalid(): void;
    onClose(): void;
  }): { close(): void };
};

const source = readFileSync(new NodeURL("../../public/landing/calculator.js", import.meta.url), "utf8");
const commonJsModule = { exports: {} };
runInNewContext(source, { module: commonJsModule, exports: commonJsModule.exports });
const calculator = commonJsModule.exports as Calculator;

afterEach(() => { document.body.innerHTML = ""; });

describe("advanced calculator bindings on the actual landing pages", () => {
  for (const language of ["ru", "en"] as const) {
    it(`updates the ${language} estimate through real controls and clears validation on close`, () => {
      const html = readFileSync(new NodeURL(`../../public/landing/${language === "ru" ? "index" : "en"}.html`, import.meta.url), "utf8");
      document.body.innerHTML = new DOMParser().parseFromString(html, "text/html").body.innerHTML;
      const app = readFileSync(new NodeURL(`../../public/landing/app${language === "en" ? "-en" : ""}.js`, import.meta.url), "utf8");
      const data = app.match(/const PROVIDERS\s*=\s*([\s\S]+?);\s*const INTEGRATIONS\s*=/);
      if (!data) throw new Error("Missing landing provider catalog");
      const providers = runInNewContext(`(${data[1]})`) as unknown[];
      const onResult = vi.fn();
      const onInvalid = vi.fn();
      const onClose = vi.fn();
      const binding = calculator.bind({ providers, language, onResult, onInvalid, onClose, document });
      const toggle = document.querySelector<HTMLButtonElement>("#advToggle")!;
      const panel = document.querySelector<HTMLElement>("#adv")!;
      const cache = document.querySelector<HTMLInputElement>("#advCache")!;
      const model = document.querySelector<HTMLSelectElement>("#advModel")!;
      const error = document.querySelector<HTMLElement>("#advError")!;

      toggle.click();
      expect(panel.hidden).toBe(false);
      expect(toggle.getAttribute("aria-expanded")).toBe("true");
      expect(onResult).toHaveBeenLastCalledWith(expect.objectContaining({ now: 113_400_000_000n, next: 56_700_000_000n }));

      cache.value = "0";
      cache.dispatchEvent(new Event("input", { bubbles: true }));
      expect(onResult).toHaveBeenLastCalledWith(expect.objectContaining({ next: 67_500_000_000n }));
      model.value = "claude-haiku-4-5";
      model.dispatchEvent(new Event("change", { bubbles: true }));
      expect(onResult).toHaveBeenLastCalledWith(expect.objectContaining({ next: 13_500_000_000n }));

      cache.value = "101%";
      cache.dispatchEvent(new Event("input", { bubbles: true }));
      expect(onInvalid).toHaveBeenCalledOnce();
      expect(onResult).toHaveBeenCalledTimes(3);
      expect(error.hidden).toBe(false);
      expect(error.textContent).not.toBe("");
      expect(cache.getAttribute("aria-invalid")).toBe("true");

      binding.close();
      expect(panel.hidden).toBe(true);
      expect(toggle.getAttribute("aria-expanded")).toBe("false");
      expect(error.hidden).toBe(true);
      expect(error.textContent).toBe("");
      expect(cache.hasAttribute("aria-invalid")).toBe(false);
      expect(onClose).not.toHaveBeenCalled();

      cache.value = "0";
      toggle.click();
      toggle.click();
      expect(panel.hidden).toBe(true);
      expect(toggle.getAttribute("aria-expanded")).toBe("false");
      expect(onClose).toHaveBeenCalledOnce();
    });
  }
});
