// @vitest-environment jsdom

import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it } from "vitest";
import { CostCalculator } from "./cost-calculator";

describe("CostCalculator", () => {
  it("switches the complete comparison between Claude and GPT models", async () => {
    const container = document.createElement("div");
    const root = createRoot(container);

    await act(async () => {
      root.render(createElement(CostCalculator));
    });

    expect(container.textContent).toContain("Every Claude model for this task");
    expect(container.textContent).toContain("claude-opus-4-8");
    expect(container.textContent).not.toContain("gpt-5.6-sol");
    expect(container.querySelector(".calc-now")?.textContent).toBe("$54.50");

    const gptButton = [...container.querySelectorAll("button")]
      .find((button) => button.textContent === "GPT models");
    expect(gptButton).toBeDefined();

    await act(async () => {
      gptButton?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    expect(container.textContent).toContain("Every GPT model for this task");
    expect(container.textContent).toContain("gpt-5.6-sol");
    expect(container.textContent).toContain("gpt-5.4");
    expect(container.textContent).not.toContain("claude-opus-4-8");
    expect(container.textContent).toContain("official OpenAI price");
    expect(container.querySelector(".calc-now")?.textContent).toBe("$59.50");
    expect(container.querySelector(".calc-was")?.textContent).toContain("$148.75");

    await act(async () => root.unmount());
    container.remove();
  });
});
