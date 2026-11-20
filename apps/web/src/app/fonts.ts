import localFont from "next/font/local";

export const ttChocolates = localFont({
  src: [
    { path: "../../public/landing/fonts/TTChocolates-Thin.woff2", weight: "200", style: "normal" },
    { path: "../../public/landing/fonts/TTChocolates-Light.woff2", weight: "300", style: "normal" },
    { path: "../../public/landing/fonts/TTChocolates-Regular.woff2", weight: "400 500", style: "normal" },
    { path: "../../public/landing/fonts/TTChocolates-Bold.woff2", weight: "600 700", style: "normal" },
    { path: "../../public/landing/fonts/TTChocolates-Black.woff2", weight: "800 900", style: "normal" },
    { path: "../../public/landing/fonts/TTChocolates-ThinItalic.woff2", weight: "200", style: "italic" },
    { path: "../../public/landing/fonts/TTChocolates-LightItalic.woff2", weight: "300", style: "italic" },
    { path: "../../public/landing/fonts/TTChocolates-RegularItalic.woff2", weight: "400 500", style: "italic" },
    { path: "../../public/landing/fonts/TTChocolates-BoldItalic.woff2", weight: "600 700", style: "italic" },
    { path: "../../public/landing/fonts/TTChocolates-BlackItalic.woff2", weight: "800 900", style: "italic" },
  ],
  variable: "--font-tt-chocolates",
  display: "swap",
  preload: true,
  fallback: ["system-ui", "-apple-system", "Segoe UI", "sans-serif"],
});

export const fontVariables = ttChocolates.variable;
