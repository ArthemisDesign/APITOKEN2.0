import type { Metadata } from "next";
import { Manrope } from "next/font/google";
import "./globals.css";

const manrope = Manrope({
  subsets: ["latin"],
  variable: "--font-manrope",
  display: "swap",
});

export const metadata: Metadata = {
  title: {
    default: "APIToken Partners — earn from every dollar your referrals spend",
    template: "%s · APIToken Partners",
  },
  description:
    "The APIToken partner program: share your link, earn a commission on every dollar your referrals spend on Claude API, and build a team of sub-partners for override earnings.",
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en" className={manrope.variable}>
      <body>{children}</body>
    </html>
  );
}
