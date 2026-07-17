import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { GitHubLogo, GoogleLogo } from "./provider-logos";

describe("authentication provider logos", () => {
  it("renders Google's approved gradient G asset without distorting its intrinsic ratio", () => {
    const markup = renderToStaticMarkup(createElement(GoogleLogo));

    expect(markup).toContain("/assets/google-g.png");
    expect(markup).toContain('width="200"');
    expect(markup).toContain('height="204"');
    expect(markup).toContain('aria-hidden="true"');
  });

  it("renders GitHub's official Invertocat vector as a theme-safe mark", () => {
    const markup = renderToStaticMarkup(createElement(GitHubLogo));

    expect(markup).toContain('viewBox="0 0 98 96"');
    expect(markup).toContain('fill="currentColor"');
    expect(markup).toContain("M41.4395 69.3848");
    expect(markup).toContain('aria-hidden="true"');
  });
});
