import { describe, expect, it, vi } from "vitest";
import { renderToString } from "react-dom/server";

vi.mock("next/link", () => ({ default: (props: { href: string; children: unknown }) => <a href={props.href}>{props.children as never}</a> }));

import PricingPage from "./page";

describe("managed pricing page", () => {
  it("renders the loading shell without issuing fetch during server render", () => {
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    const html = renderToString(<PricingPage />);
    expect(html).toContain("Pricing policies");
    expect(html).toContain("loading-grid");
    expect(fetchMock).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });
});
