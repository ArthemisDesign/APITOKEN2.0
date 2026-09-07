"use client";

import { useEffect } from "react";

/** Keep off-canvas navigation out of the tab order and lock its background. */
export function useMobileMenu(open: boolean, close: () => void, panelSelector: string, triggerSelector: string, breakpoint: number) {
  useEffect(() => {
    const panel = document.querySelector<HTMLElement>(panelSelector);
    const trigger = document.querySelector<HTMLElement>(triggerSelector);
    if (!panel || !trigger) return;
    const media = window.matchMedia(`(max-width:${breakpoint}px)`);
    const sync = () => {
      panel.inert = media.matches && !open;
      if (!media.matches && open) close();
    };
    sync();
    media.addEventListener("change", sync);
    const active = open && media.matches;
    const bodyOverflow = document.body.style.overflow;
    const htmlOverflow = document.documentElement.style.overflow;
    const content = active ? [...document.querySelectorAll<HTMLElement>("main,footer")].filter(el => !el.contains(panel)) : [];
    const inert = content.map(el => el.inert);
    if (active) {
      document.body.style.overflow = "hidden";
      document.documentElement.style.overflow = "hidden";
      content.forEach(el => { el.inert = true; });
      panel.querySelector<HTMLElement>("button,a[href]")?.focus({ preventScroll: true });
    }
    const keydown = (event: KeyboardEvent) => {
      if (!active) return;
      if (event.key === "Escape") { event.preventDefault(); close(); }
      if (event.key !== "Tab") return;
      const items = [...panel.querySelectorAll<HTMLElement>("a[href],button,input,select,textarea"), trigger]
        .filter(el => !el.matches(":disabled") && !el.closest("[inert]") && el.getClientRects().length);
      const index = items.indexOf(document.activeElement as HTMLElement);
      event.preventDefault();
      const next = index < 0 ? (event.shiftKey ? items.length - 1 : 0) : (index + (event.shiftKey ? -1 : 1) + items.length) % items.length;
      items[next]?.focus();
    };
    document.addEventListener("keydown", keydown);
    return () => {
      media.removeEventListener("change", sync);
      document.removeEventListener("keydown", keydown);
      panel.inert = false;
      if (active) {
        document.body.style.overflow = bodyOverflow;
        document.documentElement.style.overflow = htmlOverflow;
        content.forEach((el, i) => { el.inert = inert[i]!; });
        if (panel.contains(document.activeElement)) trigger.focus({ preventScroll: true });
      }
    };
  }, [open, close, panelSelector, triggerSelector, breakpoint]);
}
