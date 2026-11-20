import localFont from "next/font/local";

export const ttInterphasesPro = localFont({
  src: [
    { path: "../../public/landing/fonts/TTInterphasesPro-Variable.woff2", weight: "100 900", style: "normal" },
    { path: "../../public/landing/fonts/TTInterphasesPro-VariableItalic.woff2", weight: "100 900", style: "italic" },
  ],
  variable: "--font-tt-interphases-pro",
  display: "swap",
  preload: true,
  fallback: ["system-ui", "-apple-system", "Segoe UI", "sans-serif"],
});

export const fontVariables = ttInterphasesPro.variable;
