import { afterEach, describe, expect, it, vi } from "vitest";
import { exportJWK, generateKeyPair, SignJWT } from "jose";
import { GitHubIdentityProvider, GoogleIdentityProvider } from "./auth-providers.js";

afterEach(() => vi.unstubAllGlobals());

describe("OAuth identity providers", () => {
  it("requests Google OIDC email, state, nonce and PKCE", () => {
    const provider = new GoogleIdentityProvider({
      clientId: "google-client",
      clientSecret: "google-secret",
      redirectUri: "https://api.apitoken.sale/v1/auth/google/callback",
    });
    const url = provider.createAuthorizationUrl({ state: "state", nonce: "nonce", codeChallenge: "challenge" });
    expect(url.origin).toBe("https://accounts.google.com");
    expect(url.searchParams.get("scope")).toBe("openid email profile");
    expect(url.searchParams.get("state")).toBe("state");
    expect(url.searchParams.get("nonce")).toBe("nonce");
    expect(url.searchParams.get("code_challenge_method")).toBe("S256");
  });

  it("accepts only a cryptographically verified Google ID token with the expected nonce", async () => {
    const { publicKey, privateKey } = await generateKeyPair("RS256");
    const jwk = { ...await exportJWK(publicKey), kid: "test-key", alg: "RS256", use: "sig" };
    const idToken = await new SignJWT({
      sub: "google-subject",
      email: "google@example.com",
      email_verified: true,
      nonce: "expected-nonce",
      name: "Google User",
    })
      .setProtectedHeader({ alg: "RS256", kid: "test-key" })
      .setIssuer("https://accounts.google.com")
      .setAudience("google-client")
      .setIssuedAt()
      .setExpirationTime("5m")
      .sign(privateKey);
    vi.stubGlobal("fetch", async (input: string | URL | Request) => {
      const url = String(input);
      if (url.includes("oauth2.googleapis.com/token")) return Response.json({ id_token: idToken });
      if (url.includes("googleapis.com/oauth2/v3/certs")) return Response.json({ keys: [jwk] });
      throw new Error(`unexpected request: ${url}`);
    });
    const provider = new GoogleIdentityProvider({
      clientId: "google-client",
      clientSecret: "google-secret",
      redirectUri: "https://api.apitoken.sale/v1/auth/google/callback",
    });
    await expect(provider.exchangeCallback({
      code: "code", expectedNonce: "expected-nonce", codeVerifier: "verifier",
    })).resolves.toMatchObject({
      provider: "google", subject: "google-subject", email: "google@example.com", emailVerified: true,
    });
  });

  it("requests GitHub user:email and selects only a verified email", async () => {
    const calls: string[] = [];
    vi.stubGlobal("fetch", async (input: string | URL | Request) => {
      const url = String(input);
      calls.push(url);
      if (url.includes("access_token")) return Response.json({ access_token: "temporary-access-token" });
      if (url.endsWith("/user")) return Response.json({ id: 42, login: "octocat", name: null, avatar_url: null });
      return Response.json([
        { email: "unverified@example.com", verified: false, primary: true },
        { email: "verified@example.com", verified: true, primary: false },
      ]);
    });
    const provider = new GitHubIdentityProvider({
      clientId: "github-client",
      clientSecret: "github-secret",
      redirectUri: "https://api.apitoken.sale/v1/auth/github/callback",
    });
    const authorization = provider.createAuthorizationUrl({ state: "state", nonce: null, codeChallenge: "challenge" });
    expect(authorization.searchParams.get("scope")).toBe("user:email");
    expect(authorization.searchParams.get("code_challenge_method")).toBe("S256");

    await expect(provider.exchangeCallback({
      code: "code", expectedNonce: null, codeVerifier: "verifier",
    })).resolves.toMatchObject({
      provider: "github",
      subject: "42",
      email: "verified@example.com",
      emailVerified: true,
    });
    expect(calls).toContain("https://api.github.com/user/emails?per_page=100");
  });

  it("rejects a GitHub account when the email API returns no verified address", async () => {
    vi.stubGlobal("fetch", async (input: string | URL | Request) => {
      const url = String(input);
      if (url.includes("access_token")) return Response.json({ access_token: "temporary-access-token" });
      if (url.endsWith("/user")) return Response.json({ id: 42, login: "octocat", name: null, avatar_url: null });
      return Response.json([{ email: "unverified@example.com", verified: false, primary: true }]);
    });
    const provider = new GitHubIdentityProvider({
      clientId: "github-client",
      clientSecret: "github-secret",
      redirectUri: "https://api.apitoken.sale/v1/auth/github/callback",
    });
    await expect(provider.exchangeCallback({
      code: "code", expectedNonce: null, codeVerifier: "verifier",
    })).rejects.toThrow("no verified email");
  });
});
