// @vitest-environment jsdom

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { runInNewContext } from "node:vm";
import { describe, expect, it } from "vitest";

const source = readFileSync(resolve("public/landing/mobile-nav.js"), "utf8");

function setup(home = true) {
  const page = document.implementation.createHTMLDocument();
  if (home) page.documentElement.classList.add("landing-home");
  const root = page.documentElement;
  Object.defineProperty(page, "scrollingElement", { value: root });
  Object.defineProperties(root, {
    scrollHeight: { value: 2_000 },
    clientHeight: { value: 600 },
  });
  root.scrollTop = 300;
  runInNewContext(source, {
    document: page,
    Element,
    getComputedStyle: (node: HTMLElement) => ({
      overflowX: node.style.overflowX,
      overflowY: node.style.overflowY,
      direction: node.style.direction,
    }),
  });
  const target = page.createElement("div");
  page.body.append(target);
  function touch(node: Element, type: string, x: number, y: number, fingers = 1, cancelable = true) {
    const event = new Event(type, { bubbles: true, cancelable });
    Object.defineProperty(event, "touches", { value: Array.from({ length: fingers }, () => ({ clientX: x, clientY: y })) });
    node.dispatchEvent(event);
    return event;
  }
  const gesture = (node: Element, dx: number, dy = 0) => {
    touch(node, "touchstart", 100, 100);
    return touch(node, "touchmove", 100 + dx, 100 + dy);
  };
  function scroller(axis: "x" | "y", position: number, direction = "ltr") {
    const box = page.createElement("div");
    if (axis === "x") box.style.overflowX = "auto";
    else box.style.overflowY = "scroll";
    box.style.direction = direction;
    Object.defineProperties(box, {
      scrollWidth: { value: axis === "x" ? 600 : 200 },
      clientWidth: { value: 200 },
      scrollHeight: { value: axis === "y" ? 600 : 200 },
      clientHeight: { value: 200 },
    });
    box.scrollLeft = axis === "x" ? position : 0;
    box.scrollTop = axis === "y" ? position : 0;
    page.body.append(box);
    box.append(target);
    return box;
  }
  return { page, root, target, touch, gesture, scroller };
}

describe("landing touch scroll containment", () => {
  it.each([-40, 40])("blocks sideways page movement in direction %s", (dx) => {
    const { target, gesture } = setup();
    expect(gesture(target, dx).defaultPrevented).toBe(true);
    expect(gesture(target, dx, 4).defaultPrevented).toBe(true);
  });

  it.each([-40, 40])("preserves an inner horizontal scroller in the middle (%s)", (dx) => {
    const { target, gesture, scroller } = setup();
    scroller("x", 200);
    expect(gesture(target, dx).defaultPrevented).toBe(false);
  });

  it("blocks page chaining at each horizontal edge while allowing the reverse direction", () => {
    const { target, gesture, scroller } = setup();
    const box = scroller("x", 0);
    expect(gesture(target, 40).defaultPrevented).toBe(true);
    expect(gesture(target, -40).defaultPrevented).toBe(false);
    box.scrollLeft = 400;
    expect(gesture(target, -40).defaultPrevented).toBe(true);
    expect(gesture(target, 40).defaultPrevented).toBe(false);
    box.scrollLeft = -5; // Safari elastic position beyond the left edge.
    expect(gesture(target, 40).defaultPrevented).toBe(true);
    box.scrollLeft = 405;
    expect(gesture(target, -40).defaultPrevented).toBe(true);
  });

  it("uses the actual direction of an RTL horizontal scroller", () => {
    const { target, gesture, scroller } = setup();
    const box = scroller("x", 0, "rtl");
    expect(gesture(target, -40).defaultPrevented).toBe(true);
    expect(gesture(target, 40).defaultPrevented).toBe(false);
    box.scrollLeft = -400;
    expect(gesture(target, 40).defaultPrevented).toBe(true);
    expect(gesture(target, -40).defaultPrevented).toBe(false);
  });

  it("does not treat overflowing but clipped content as an inner scroller", () => {
    const { target, gesture, scroller } = setup();
    const box = scroller("x", 200);
    box.style.overflowX = "hidden";
    expect(gesture(target, -40).defaultPrevented).toBe(true);
  });

  it("allows a surrounding inner scroller to consume a nested boundary gesture", () => {
    const { page, target, gesture, scroller } = setup();
    const inner = scroller("x", 0);
    const outer = page.createElement("div");
    outer.style.overflowX = "auto";
    Object.defineProperties(outer, { scrollWidth: { value: 800 }, clientWidth: { value: 300 } });
    outer.scrollLeft = 200;
    page.body.append(outer);
    outer.append(inner);
    expect(gesture(target, 40).defaultPrevented).toBe(false);
    outer.scrollLeft = 0;
    expect(gesture(target, 40).defaultPrevented).toBe(true);
  });

  it("preserves vertical page and inner scrolling with the existing boundary guard", () => {
    const { root, target, gesture, scroller } = setup();
    expect(gesture(target, 4, -40).defaultPrevented).toBe(false);
    expect(gesture(target, 4, 40).defaultPrevented).toBe(false);
    root.scrollTop = 0;
    expect(gesture(target, 0, 40).defaultPrevented).toBe(true);
    expect(gesture(target, 0, -40).defaultPrevented).toBe(false);
    const box = scroller("y", 200);
    expect(gesture(target, 0, 40).defaultPrevented).toBe(false);
    box.scrollTop = 0;
    expect(gesture(target, 0, 40).defaultPrevented).toBe(true);
    root.scrollTop = 1_400;
    expect(gesture(target, 0, -40).defaultPrevented).toBe(false);
    box.scrollTop = 400;
    expect(gesture(target, 0, -40).defaultPrevented).toBe(true);
  });

  it("keeps a vertical swipe native when later moves contain sideways jitter", () => {
    const { target, touch } = setup();
    touch(target, "touchstart", 100, 100);
    expect(touch(target, "touchmove", 102, 99).defaultPrevented).toBe(false);
    expect(touch(target, "touchmove", 104, 60).defaultPrevented).toBe(false);
    expect(touch(target, "touchmove", 108, 59).defaultPrevented).toBe(false);
    expect(touch(target, "touchmove", 112, 59).defaultPrevented).toBe(false);
  });

  it("blocks a horizontal swipe even when a later move has more vertical jitter", () => {
    const { target, touch } = setup();
    touch(target, "touchstart", 100, 100);
    expect(touch(target, "touchmove", 140, 104).defaultPrevented).toBe(true);
    expect(touch(target, "touchmove", 141, 108).defaultPrevented).toBe(true);
  });

  it.each(["input", "textarea", "select", "contenteditable"])("preserves native %s gestures", (tag) => {
    const { page, target, gesture } = setup();
    const field = page.createElement(tag === "contenteditable" ? "div" : tag);
    if (tag === "contenteditable") field.setAttribute("contenteditable", "true");
    target.append(field);
    expect(gesture(field, 40).defaultPrevented).toBe(false);
  });

  it("preserves pinch zoom and clears the single-finger gesture after it", () => {
    const { target, touch } = setup();
    touch(target, "touchstart", 100, 100, 2);
    expect(touch(target, "touchmove", 140, 100, 2).defaultPrevented).toBe(false);
    expect(touch(target, "touchmove", 180, 100, 1).defaultPrevented).toBe(false);
    touch(target, "touchstart", 100, 100);
    expect(touch(target, "touchmove", 140, 100, 2).defaultPrevented).toBe(false);
    expect(touch(target, "touchmove", 180, 100).defaultPrevented).toBe(false);
  });

  it.each(["touchend", "touchcancel"])("clears gesture tracking on %s", (type) => {
    const { target, touch } = setup();
    touch(target, "touchstart", 100, 100);
    touch(target, type, 100, 100, 0);
    expect(touch(target, "touchmove", 140, 100).defaultPrevented).toBe(false);
  });

  it("does not add the home-only guard to other pages or cancel non-cancelable moves", () => {
    const { target, gesture } = setup(false);
    expect(gesture(target, 40).defaultPrevented).toBe(false);
    const home = setup();
    home.touch(home.target, "touchstart", 100, 100);
    expect(home.touch(home.target, "touchmove", 140, 100, 1, false).defaultPrevented).toBe(false);
  });
});
