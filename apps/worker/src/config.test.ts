import assert from "node:assert/strict";
import test from "node:test";
import { validateEnvironment } from "./config.js";
import { smtpSecurityOptions } from "./smtp.js";

const productionEnvironment = {
  NODE_ENV: "production",
  DATABASE_URL: "postgresql://commerce:secret@127.0.0.1:5432/commerce",
  ENGINE_BASE_URL: "http://127.0.0.1:8790",
  ENGINE_CONTROL_KEY: "c".repeat(32),
  AUTH_TOKEN_ENCRYPTION_KEY: "a".repeat(43),
  EMAIL_DELIVERY_MODE: "smtp",
  EMAIL_FROM: "no-reply@apitoken.sale",
  SMTP_HOST: "smtp-relay.brevo.com",
  SMTP_PORT: "587",
  SMTP_SECURE: "false",
  SMTP_USERNAME: "smtp-login@example.com",
  SMTP_PASSWORD: "smtp-key",
  PUBLIC_APP_BASE_URL: "https://apitoken.sale",
} as const;

test("accepts authenticated Brevo STARTTLS configuration in production", () => {
  const environment = validateEnvironment(productionEnvironment);

  assert.equal(environment.SMTP_PORT, 587);
  assert.equal(environment.SMTP_SECURE, false);
});

test("requires STARTTLS for explicit-TLS SMTP in production", () => {
  assert.deepEqual(smtpSecurityOptions({ NODE_ENV: "production", SMTP_SECURE: false }), {
    secure: false,
    requireTLS: true,
    tls: { minVersion: "TLSv1.2" },
  });
});

test("uses implicit TLS without STARTTLS on port 465 configurations", () => {
  assert.deepEqual(smtpSecurityOptions({ NODE_ENV: "production", SMTP_SECURE: true }), {
    secure: true,
    requireTLS: false,
    tls: { minVersion: "TLSv1.2" },
  });
});

test("allows a non-TLS local capture server outside production", () => {
  assert.equal(
    smtpSecurityOptions({ NODE_ENV: "development", SMTP_SECURE: false }).requireTLS,
    false,
  );
});
