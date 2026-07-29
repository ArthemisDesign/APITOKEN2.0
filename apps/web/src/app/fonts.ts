import localFont from "next/font/local";

export const jetBrainsMono = localFont({
  src: "./fonts/jetbrains-mono-variable.woff2",
  variable: "--font-jetbrains-mono",
  weight: "100 800",
  style: "normal",
  display: "swap",
  preload: true,
  fallback: ["ui-monospace", "SFMono-Regular", "Menlo", "Monaco", "Consolas", "monospace"],
});

export const handjet = localFont({
  src: "./fonts/handjet-variable.woff2",
  variable: "--font-handjet",
  weight: "100 900",
  style: "normal",
  display: "optional",
  preload: true,
  fallback: ["ui-monospace", "SFMono-Regular", "Menlo", "Monaco", "Consolas", "monospace"],
  adjustFontFallback: false,
});

export const bitcountGrid = localFont({
  src: "./fonts/bitcount-grid-single.woff2",
  variable: "--font-bitcount-grid",
  weight: "400",
  style: "normal",
  display: "optional",
  preload: false,
  fallback: ["ui-monospace", "SFMono-Regular", "Menlo", "Monaco", "Consolas", "monospace"],
  adjustFontFallback: false,
});

export const fontVariables = `${jetBrainsMono.variable} ${handjet.variable} ${bitcountGrid.variable}`;
