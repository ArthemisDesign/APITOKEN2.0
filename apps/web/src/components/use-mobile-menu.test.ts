// @vitest-environment jsdom

import { act, createElement, Fragment } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useMobileMenu } from "./use-mobile-menu";

const close = vi.fn();

function Menu({ open, removeScrim = false }: { open: boolean; removeScrim?: boolean }) {
  useMobileMenu(open, close, "#test-navigation", ".test-trigger", 1080);
  return createElement(Fragment, null,
    createElement("button", { className: "test-trigger", "aria-expanded": open }, "Menu"),
    createElement("aside", { id: "test-navigation" },
      createElement("button", { onClick: close }, "Close menu")),
    (!removeScrim || open) && createElement("button", {
      className: "test-scrim", "aria-hidden": !open, tabIndex: open ? 0 : -1, onClick: close,
    }, "Close menu"),
    createElement("main", { tabIndex: -1 }, "Destination"),
  );
}

describe("useMobileMenu focus restoration", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.stubGlobal("matchMedia", vi.fn(() => ({
      matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn(),
    })));
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    document.body.style.overflow = "auto";
    document.documentElement.style.overflow = "scroll";
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    document.body.style.overflow = "";
    document.documentElement.style.overflow = "";
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  async function render(open: boolean, removeScrim = false) {
    await act(async () => root.render(createElement(Menu, { open, removeScrim })));
  }

  it("returns focus from a dismissed hidden scrim and restores background scrolling", async () => {
    await render(false);
    const main = container.querySelector("main")!;
    main.inert = false;
    await render(true);
    expect(main.inert).toBe(true);
    expect(document.body.style.overflow).toBe("hidden");
    container.querySelector<HTMLButtonElement>(".test-scrim")!.focus();

    await render(false);

    expect(document.activeElement).toBe(container.querySelector(".test-trigger"));
    expect(main.inert).toBe(false);
    expect(document.body.style.overflow).toBe("auto");
    expect(document.documentElement.style.overflow).toBe("scroll");
  });

  it("returns focus when the focused scrim is removed from the document", async () => {
    await render(true, true);
    container.querySelector<HTMLButtonElement>(".test-scrim")!.focus();

    await render(false, true);

    expect(container.querySelector(".test-scrim")).toBeNull();
    expect(document.activeElement).toBe(container.querySelector(".test-trigger"));
  });

  it("preserves deliberate focus on the navigation destination", async () => {
    await render(true);
    const main = container.querySelector("main")!;
    main.inert = false;
    main.focus();

    await render(false);

    expect(document.activeElement).toBe(main);
  });
});
