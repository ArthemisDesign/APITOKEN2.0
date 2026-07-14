"use client";

import type { ElementType, HTMLAttributes, ReactNode } from "react";
import { useI18n } from "./i18n-provider";

export function T({ k, as: Tag = "span", children, ...props }: {
  k: string;
  as?: ElementType;
  children?: ReactNode;
} & HTMLAttributes<HTMLElement>) {
  const { t } = useI18n();
  return <Tag {...props} data-i18n-key={k}>{t(k) === k ? children : t(k)}</Tag>;
}
