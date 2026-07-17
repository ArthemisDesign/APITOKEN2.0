import type { AuthEmailPurpose } from "@claude-api/db";

export interface RenderedAuthEmail {
  subject: string;
  text: string;
  html: string;
}

interface TemplateCopy {
  subject: string;
  preheader: string;
  eyebrow: string;
  heading: string;
  introduction: string;
  action: string;
  securityNote: string;
}

const COPY: Record<AuthEmailPurpose, TemplateCopy> = {
  verify_email: {
    subject: "Verify your email for apiToken.sale",
    preheader: "Confirm your email address to finish setting up your apiToken.sale account.",
    eyebrow: "Account verification",
    heading: "Verify your email address",
    introduction: "Thanks for creating an apiToken.sale account. Confirm this email address to finish setting it up and sign in.",
    action: "Verify email address",
    securityNote: "If you did not create an apiToken.sale account, you can safely ignore this email.",
  },
  reset_password: {
    subject: "Reset your apiToken.sale password",
    preheader: "Use this secure link to choose a new password for your apiToken.sale account.",
    eyebrow: "Password reset",
    heading: "Choose a new password",
    introduction: "We received a request to reset the password for your apiToken.sale account. Use the secure link below to choose a new one.",
    action: "Reset password",
    securityNote: "If you did not request a password reset, ignore this email. Your password will remain unchanged.",
  },
};

export function renderAuthEmail(
  purpose: AuthEmailPurpose,
  token: string,
  appBaseUrl: string,
): RenderedAuthEmail {
  const copy = COPY[purpose];
  const path = purpose === "verify_email" ? "/verify-email" : "/reset-password";
  const url = new URL(path, appBaseUrl);
  url.searchParams.set("token", token);
  const actionUrl = url.toString();
  const escapedUrl = escapeHtml(actionUrl);

  return {
    subject: copy.subject,
    text: [
      copy.heading,
      "",
      copy.introduction,
      "",
      `${copy.action}:`,
      actionUrl,
      "",
      "This secure link can be used once and expires automatically.",
      "",
      copy.securityNote,
      "",
      "apiToken.sale",
      "https://apitoken.sale",
    ].join("\n"),
    html: `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width,initial-scale=1">
    <meta name="color-scheme" content="light">
    <title>${escapeHtml(copy.subject)}</title>
  </head>
  <body style="margin:0;padding:0;background:#f5f5f4;color:#63625c;font-family:'JetBrains Mono','SFMono-Regular',Consolas,'Liberation Mono',monospace;">
    <div style="display:none;max-height:0;overflow:hidden;opacity:0;color:transparent;">${escapeHtml(copy.preheader)}</div>
    <table role="presentation" width="100%" cellspacing="0" cellpadding="0" border="0" style="width:100%;background-color:#f5f5f4;background-image:linear-gradient(rgba(10,10,10,.035) 1px,transparent 1px),linear-gradient(90deg,rgba(10,10,10,.035) 1px,transparent 1px);background-size:44px 44px;">
      <tr>
        <td align="center" style="padding:32px 16px;">
          <table role="presentation" width="100%" cellspacing="0" cellpadding="0" border="0" style="width:100%;max-width:600px;background:#ffffff;border:1px solid #dededb;border-radius:10px;overflow:hidden;box-shadow:0 1px 2px rgba(10,10,10,.04),0 8px 30px rgba(10,10,10,.06);">
            <tr>
              <td style="padding:20px 32px;border-bottom:1px solid #dededb;background:#ffffff;color:#0a0a0a;font-size:20px;line-height:24px;font-weight:700;letter-spacing:.04em;">
                <a href="https://apitoken.sale" style="color:#0a0a0a;text-decoration:none;white-space:nowrap;"><img src="https://apitoken.sale/assets/logo-mark-light.png" width="22" height="22" alt="" style="display:inline-block;width:22px;height:22px;margin:0 10px 0 0;border:0;vertical-align:-5px;">apiToken.sale</a>
              </td>
            </tr>
            <tr>
              <td style="padding:36px 32px 32px;">
                <p style="margin:0 0 14px;color:#3767f0;font-size:13px;line-height:18px;font-weight:600;letter-spacing:.05em;text-transform:uppercase;">—&nbsp;&nbsp;${escapeHtml(copy.eyebrow)}</p>
                <h1 style="margin:0 0 16px;color:#0a0a0a;font-size:28px;line-height:35px;font-weight:700;letter-spacing:0;">${escapeHtml(copy.heading)}</h1>
                <p style="margin:0 0 28px;color:#63625c;font-size:15px;line-height:24px;">${escapeHtml(copy.introduction)}</p>
                <table role="presentation" cellspacing="0" cellpadding="0" border="0">
                  <tr>
                    <td style="border:1px solid #3767f0;border-radius:4px;background:#3767f0;">
                      <a href="${escapedUrl}" style="display:inline-block;padding:12px 22px;color:#ffffff;text-decoration:none;font-size:15px;line-height:22px;font-weight:600;letter-spacing:.01em;">${escapeHtml(copy.action)}</a>
                    </td>
                  </tr>
                </table>
                <p style="margin:26px 0 8px;color:#73726c;font-size:12px;line-height:19px;">If the button does not work, copy and paste this link into your browser:</p>
                <p style="margin:0;word-break:break-all;font-size:12px;line-height:19px;"><a href="${escapedUrl}" style="color:#3767f0;text-decoration:underline;">${escapedUrl}</a></p>
                <div style="margin-top:28px;padding:16px 18px;background:#f5f5f4;border:1px solid #dededb;border-radius:4px;">
                  <p style="margin:0 0 6px;color:#0a0a0a;font-size:13px;line-height:20px;font-weight:600;">Security notice</p>
                  <p style="margin:0;color:#696862;font-size:12px;line-height:19px;">This secure link can be used once and expires automatically. ${escapeHtml(copy.securityNote)}</p>
                </div>
              </td>
            </tr>
            <tr>
              <td style="padding:20px 32px;border-top:1px solid #dededb;background:#fbfbf9;color:#73726c;font-size:11px;line-height:18px;">
                This is an automated account-security email from <a href="https://apitoken.sale" style="color:#63625c;text-decoration:underline;">apiToken.sale</a>. Please do not reply.
              </td>
            </tr>
          </table>
        </td>
      </tr>
    </table>
  </body>
</html>`,
  };
}

function escapeHtml(value: string): string {
  return value.replace(/[&<>"']/g, (character) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
  })[character]!);
}
