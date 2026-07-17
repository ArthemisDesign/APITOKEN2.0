import assert from "node:assert/strict";
import test from "node:test";
import { renderAuthEmail } from "./email-template.js";

const token = "a".repeat(43);

test("renders a branded email-verification message with HTML and plain-text fallbacks", () => {
  const message = renderAuthEmail("verify_email", token, "https://apitoken.sale");

  assert.equal(message.subject, "Verify your email for apiToken.sale");
  assert.match(message.text, /Thanks for creating an apiToken\.sale account/);
  assert.match(message.text, new RegExp(`https://apitoken\\.sale/verify-email\\?token=${token}`));
  assert.match(message.html, /Verify email address/);
  assert.match(message.html, /Security notice/);
  assert.match(message.html, /font-family:'JetBrains Mono'/);
  assert.match(message.html, /background:#3767f0/);
  assert.match(message.html, /logo-mark-light\.png/);
  assert.match(message.html, new RegExp(`/verify-email\\?token=${token}`));
  assert.doesNotMatch(message.html, /Reset password/);
});

test("renders a password-reset message without implying that the password already changed", () => {
  const message = renderAuthEmail("reset_password", token, "https://apitoken.sale");

  assert.equal(message.subject, "Reset your apiToken.sale password");
  assert.match(message.text, /Your password will remain unchanged/);
  assert.match(message.text, new RegExp(`https://apitoken\\.sale/reset-password\\?token=${token}`));
  assert.match(message.html, /Choose a new password/);
  assert.match(message.html, /Your password will remain unchanged/);
  assert.doesNotMatch(message.html, /Thanks for creating/);
});

test("escapes generated link markup", () => {
  const message = renderAuthEmail("verify_email", 'unsafe"&token', "https://apitoken.sale");

  assert.doesNotMatch(message.html, /unsafe"&token/);
  assert.match(message.html, /unsafe%22%26token/);
});
