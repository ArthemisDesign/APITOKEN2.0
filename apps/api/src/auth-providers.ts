import { createRemoteJWKSet, jwtVerify } from "jose";
import { z } from "zod";
import type { OAuthProvider, VerifiedExternalIdentity } from "@claude-api/db";

export interface OAuthAuthorizationInput {
  state: string;
  nonce: string | null;
  codeChallenge: string;
}

export interface OAuthCallbackInput {
  code: string;
  expectedNonce: string | null;
  codeVerifier: string;
}

export interface ExternalIdentityProvider {
  readonly code: OAuthProvider;
  createAuthorizationUrl(input: OAuthAuthorizationInput): URL;
  exchangeCallback(input: OAuthCallbackInput): Promise<VerifiedExternalIdentity>;
}

export class OAuthProviderError extends Error {
  constructor(message: string, readonly retryable = false) {
    super(message);
    this.name = "OAuthProviderError";
  }
}

export class OAuthProviderRegistry {
  private readonly providers = new Map<OAuthProvider, ExternalIdentityProvider>();

  constructor(providers: readonly ExternalIdentityProvider[]) {
    for (const provider of providers) this.providers.set(provider.code, provider);
  }

  has(code: OAuthProvider): boolean {
    return this.providers.has(code);
  }

  get(code: OAuthProvider): ExternalIdentityProvider {
    const provider = this.providers.get(code);
    if (!provider) throw new OAuthProviderError(`${code} authentication is not configured`);
    return provider;
  }
}

export class GoogleIdentityProvider implements ExternalIdentityProvider {
  readonly code = "google" as const;
  private readonly jwks = createRemoteJWKSet(new URL("https://www.googleapis.com/oauth2/v3/certs"));

  constructor(private readonly options: { clientId: string; clientSecret: string; redirectUri: string }) {}

  createAuthorizationUrl(input: OAuthAuthorizationInput): URL {
    if (!input.nonce) throw new OAuthProviderError("Google authentication requires a nonce");
    const url = new URL("https://accounts.google.com/o/oauth2/v2/auth");
    url.searchParams.set("response_type", "code");
    url.searchParams.set("client_id", this.options.clientId);
    url.searchParams.set("redirect_uri", this.options.redirectUri);
    url.searchParams.set("scope", "openid email profile");
    url.searchParams.set("state", input.state);
    url.searchParams.set("nonce", input.nonce);
    url.searchParams.set("code_challenge", input.codeChallenge);
    url.searchParams.set("code_challenge_method", "S256");
    url.searchParams.set("prompt", "select_account");
    return url;
  }

  async exchangeCallback(input: OAuthCallbackInput): Promise<VerifiedExternalIdentity> {
    if (!input.expectedNonce) throw new OAuthProviderError("Google authentication transaction has no nonce");
    const response = await fetch("https://oauth2.googleapis.com/token", {
      method: "POST",
      headers: { "content-type": "application/x-www-form-urlencoded", accept: "application/json" },
      body: new URLSearchParams({
        code: input.code,
        client_id: this.options.clientId,
        client_secret: this.options.clientSecret,
        redirect_uri: this.options.redirectUri,
        grant_type: "authorization_code",
        code_verifier: input.codeVerifier,
      }),
    });
    if (!response.ok) throw new OAuthProviderError(`Google token exchange failed with HTTP ${response.status}`, response.status >= 500);
    const token = googleTokenSchema.safeParse(await response.json());
    if (!token.success) throw new OAuthProviderError("Google returned an invalid token response", true);
    const verified = await jwtVerify(token.data.id_token, this.jwks, {
      issuer: ["https://accounts.google.com", "accounts.google.com"],
      audience: this.options.clientId,
    });
    const claims = googleClaimsSchema.safeParse(verified.payload);
    if (!claims.success || claims.data.nonce !== input.expectedNonce || !claims.data.email_verified) {
      throw new OAuthProviderError("Google identity is missing a verified email or valid nonce");
    }
    return {
      provider: "google",
      subject: claims.data.sub,
      email: claims.data.email.toLowerCase(),
      emailVerified: true,
      displayName: claims.data.name ?? null,
      metadata: { picture: claims.data.picture ?? null },
    };
  }
}

export class GitHubIdentityProvider implements ExternalIdentityProvider {
  readonly code = "github" as const;

  constructor(private readonly options: { clientId: string; clientSecret: string; redirectUri: string }) {}

  createAuthorizationUrl(input: OAuthAuthorizationInput): URL {
    const url = new URL("https://github.com/login/oauth/authorize");
    url.searchParams.set("client_id", this.options.clientId);
    url.searchParams.set("redirect_uri", this.options.redirectUri);
    url.searchParams.set("scope", "user:email");
    url.searchParams.set("state", input.state);
    url.searchParams.set("code_challenge", input.codeChallenge);
    url.searchParams.set("code_challenge_method", "S256");
    url.searchParams.set("prompt", "select_account");
    return url;
  }

  async exchangeCallback(input: OAuthCallbackInput): Promise<VerifiedExternalIdentity> {
    const tokenResponse = await fetch("https://github.com/login/oauth/access_token", {
      method: "POST",
      headers: { accept: "application/json", "content-type": "application/x-www-form-urlencoded" },
      body: new URLSearchParams({
        client_id: this.options.clientId,
        client_secret: this.options.clientSecret,
        code: input.code,
        redirect_uri: this.options.redirectUri,
        code_verifier: input.codeVerifier,
      }),
    });
    if (!tokenResponse.ok) throw new OAuthProviderError(`GitHub token exchange failed with HTTP ${tokenResponse.status}`, tokenResponse.status >= 500);
    const token = githubTokenSchema.safeParse(await tokenResponse.json());
    if (!token.success || token.data.error) throw new OAuthProviderError(token.success ? token.data.error! : "GitHub returned an invalid token response");
    const headers = {
      accept: "application/vnd.github+json",
      authorization: `Bearer ${token.data.access_token!}`,
      "user-agent": "apitoken.sale",
      "x-github-api-version": "2022-11-28",
    };
    const [userResponse, emailsResponse] = await Promise.all([
      fetch("https://api.github.com/user", { headers }),
      fetch("https://api.github.com/user/emails?per_page=100", { headers }),
    ]);
    if (!userResponse.ok || !emailsResponse.ok) {
      throw new OAuthProviderError("GitHub profile or verified email lookup failed", userResponse.status >= 500 || emailsResponse.status >= 500);
    }
    const user = githubUserSchema.safeParse(await userResponse.json());
    const emails = githubEmailsSchema.safeParse(await emailsResponse.json());
    if (!user.success || !emails.success) throw new OAuthProviderError("GitHub returned invalid identity data", true);
    const selected = emails.data.find((email) => email.primary && email.verified) ??
      emails.data.find((email) => email.verified);
    if (!selected) throw new OAuthProviderError("GitHub account has no verified email address");
    return {
      provider: "github",
      subject: String(user.data.id),
      email: selected.email.toLowerCase(),
      emailVerified: true,
      displayName: user.data.name ?? user.data.login,
      metadata: { login: user.data.login, avatarUrl: user.data.avatar_url ?? null },
    };
  }
}

const googleTokenSchema = z.object({ id_token: z.string().min(1) });
const googleClaimsSchema = z.object({
  sub: z.string().min(1),
  email: z.string().email(),
  email_verified: z.boolean(),
  nonce: z.string().min(1),
  name: z.string().optional(),
  picture: z.string().url().optional(),
});
const githubTokenSchema = z.object({
  access_token: z.string().min(1).optional(),
  error: z.string().optional(),
}).refine((value) => Boolean(value.access_token) !== Boolean(value.error));
const githubUserSchema = z.object({
  id: z.number().int().safe().positive(),
  login: z.string().min(1),
  name: z.string().nullable(),
  avatar_url: z.string().url().nullable(),
});
const githubEmailsSchema = z.array(z.object({
  email: z.string().email(),
  verified: z.boolean(),
  primary: z.boolean(),
}));
