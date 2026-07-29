// @vitest-environment jsdom

import { act, createElement, type ComponentType, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
  class MockApiError extends Error {
    constructor(message: string, readonly status: number) {
      super(message);
    }
  }

  return {
    me: vi.fn(),
    replace: vi.fn(),
    MockApiError,
  };
});

vi.mock("next/navigation", () => ({
  useRouter: () => ({ replace: mocks.replace }),
}));

vi.mock("@/lib/api", () => ({
  api: { me: mocks.me },
  ApiError: mocks.MockApiError,
}));

import { AuthEntryGuard } from "./auth-entry-guard";

const TestableAuthEntryGuard = AuthEntryGuard as ComponentType<{
  children?: ReactNode;
  dashboardHref: string;
  language: "en" | "ru";
}>;

describe("AuthEntryGuard", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    vi.clearAllMocks();
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
  });

  it("keeps the auth form hidden while the session check is pending, then redirects an authenticated user", async () => {
    let resolveSession!: (value: unknown) => void;
    mocks.me.mockReturnValue(new Promise((resolve) => {
      resolveSession = resolve;
    }));

    await renderGuard(root);

    expect(container.textContent).toContain("Checking your session");
    expect(container.textContent).not.toContain("login form");

    await act(async () => resolveSession({ user: { id: "user-1" } }));

    expect(mocks.replace).toHaveBeenCalledWith("/dashboard");
    expect(container.textContent).not.toContain("login form");
  });

  it("shows the auth form only after the API confirms there is no session", async () => {
    mocks.me.mockRejectedValue(new mocks.MockApiError("Authentication required", 401));

    await renderGuard(root);

    expect(container.textContent).toContain("login form");
    expect(mocks.replace).not.toHaveBeenCalled();
  });

  it("does not expose the auth form when the session check fails unexpectedly", async () => {
    mocks.me.mockRejectedValue(new mocks.MockApiError("Service unavailable", 503));

    await renderGuard(root);

    expect(container.textContent).toContain("We couldn’t check your session");
    expect(container.textContent).not.toContain("login form");
    expect(mocks.replace).not.toHaveBeenCalled();
  });
});

async function renderGuard(root: Root): Promise<void> {
  await act(async () => {
    root.render(createElement(
      TestableAuthEntryGuard,
      {
        dashboardHref: "/dashboard",
        language: "en",
      },
      createElement("div", null, "login form"),
    ));
  });
}
