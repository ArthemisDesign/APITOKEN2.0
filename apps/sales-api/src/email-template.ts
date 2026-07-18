import type { PartnerAuthPurpose } from "@claude-api/sales-db";

export interface RenderedPartnerEmail {
  subject: string;
  text: string;
  html: string;
}

interface TemplateCopy {
  subject: string;
  heading: string;
  introduction: string;
  action: string;
  securityNote: string;
}

const COPY: Record<PartnerAuthPurpose, TemplateCopy> = {
  verify_email: {
    subject: "Verify your email for APIToken Partners",
    heading: "Verify your email address",
    introduction: "Thanks for joining the APIToken partner program. Confirm this email address to activate your partner account.",
    action: "Verify email address",
    securityNote: "If you did not create an APIToken Partners account, you can safely ignore this email.",
  },
  reset_password: {
    subject: "Reset your APIToken Partners password",
    heading: "Choose a new password",
    introduction: "We received a request to reset the password for your APIToken Partners account. Use the secure link below to choose a new one.",
    action: "Reset password",
    securityNote: "If you did not request a password reset, ignore this email. Your password will remain unchanged.",
  },
};

export function renderPartnerEmail(
  purpose: PartnerAuthPurpose,
  token: string,
  salesBaseUrl: string,
): RenderedPartnerEmail {
  const copy = COPY[purpose];
  const path = purpose === "verify_email" ? "/verify-email" : "/reset-password";
  const url = new URL(path, salesBaseUrl);
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
      "APIToken Partners",
    ].join("\n"),
    html: `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width,initial-scale=1">
    <title>${escapeHtml(copy.subject)}</title>
  </head>
  <body style="margin:0;padding:0;background:#f5f5f4;color:#3d3d3a;font-family:-apple-system,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;">
    <table role="presentation" width="100%" cellspacing="0" cellpadding="0" border="0" style="width:100%;background:#f5f5f4;">
      <tr>
        <td align="center" style="padding:32px 16px;">
          <table role="presentation" width="100%" cellspacing="0" cellpadding="0" border="0" style="width:100%;max-width:560px;background:#ffffff;border:1px solid #dededb;border-radius:8px;overflow:hidden;">
            <tr>
              <td style="padding:20px 28px;border-bottom:1px solid #dededb;color:#0a0a0a;font-size:16px;font-weight:700;letter-spacing:.02em;">
                APIToken Partners
              </td>
            </tr>
            <tr>
              <td style="padding:32px 28px;">
                <h1 style="margin:0 0 14px;color:#0a0a0a;font-size:22px;line-height:28px;">${escapeHtml(copy.heading)}</h1>
                <p style="margin:0 0 24px;font-size:15px;line-height:23px;">${escapeHtml(copy.introduction)}</p>
                <table role="presentation" cellspacing="0" cellpadding="0" border="0">
                  <tr>
                    <td style="border-radius:6px;background:#3767f0;">
                      <a href="${escapedUrl}" style="display:inline-block;padding:11px 20px;color:#ffffff;text-decoration:none;font-size:14px;font-weight:600;">${escapeHtml(copy.action)}</a>
                    </td>
                  </tr>
                </table>
                <p style="margin:24px 0 6px;color:#73726c;font-size:12px;line-height:18px;">If the button does not work, copy and paste this link into your browser:</p>
                <p style="margin:0;word-break:break-all;font-size:12px;line-height:18px;"><a href="${escapedUrl}" style="color:#3767f0;">${escapedUrl}</a></p>
                <p style="margin:24px 0 0;color:#73726c;font-size:12px;line-height:18px;">This secure link can be used once and expires automatically. ${escapeHtml(copy.securityNote)}</p>
              </td>
            </tr>
            <tr>
              <td style="padding:16px 28px;border-top:1px solid #dededb;color:#73726c;font-size:11px;line-height:17px;">
                This is an automated account-security email from APIToken Partners. Please do not reply.
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
