export interface EmailMessage {
  recipient: string;
  template: "verify_email" | "reset_password";
  variables: Readonly<Record<string, string>>;
}

/** Future SMTP/transactional-email boundary. The database outbox remains provider-independent. */
export interface EmailDeliveryProvider {
  readonly code: string;
  send(message: EmailMessage): Promise<{ providerMessageId: string }>;
}

export interface ExternalIdentity {
  provider: "google";
  subject: string;
  email: string;
  emailVerified: boolean;
  displayName: string | null;
}

/** Future Google OIDC boundary. Implementations must verify state, nonce, issuer, audience and signature. */
export interface GoogleIdentityProvider {
  createAuthorizationUrl(input: { state: string; nonce: string }): URL;
  exchangeCallback(input: { code: string; expectedNonce: string }): Promise<ExternalIdentity>;
}
