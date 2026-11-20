import localFont from "next/font/local";

export const ttFirsNeue = localFont({
  src: [
    { path: "../../public/landing/fonts/TTFirsNeue-Variable.woff2", weight: "100 900", style: "normal" },
    { path: "../../public/landing/fonts/TTFirsNeue-VariableItalic.woff2", weight: "100 900", style: "italic" },
  ],
  variable: "--font-tt-firs-neue",
  display: "swap",
  preload: true,
  fallback: ["system-ui", "-apple-system", "Segoe UI", "sans-serif"],
});

export const fontVariables = ttFirsNeue.variable;
