"use client";

import { createContext, useContext } from "react";
import type { Partner } from "@/lib/api";

export const PartnerContext = createContext<Partner | null>(null);

export function usePartner(): Partner {
  const partner = useContext(PartnerContext);
  if (!partner) throw new Error("usePartner must be used inside the dashboard layout");
  return partner;
}
