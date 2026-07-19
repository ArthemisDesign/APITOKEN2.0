import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: {
    default: "APIToken Partners — earn from every dollar your referrals spend",
    template: "%s · APIToken Partners",
  },
  description:
    "Partner program for apitoken.sale: share your link and earn a percentage of what your referrals actually spend on Claude API usage (their real paid usage, after discount).",
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
