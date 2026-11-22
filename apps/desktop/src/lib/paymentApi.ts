// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { invoke } from "@tauri-apps/api/core";
import type { PaymentMethod } from "./models";
export type PaymentOption = {
  id: string | null; name: string; kind: PaymentMethod; provider: string; merchantAccount: string;
  terminalId: string; instructions: string; referenceRequired: boolean; enabled: boolean;
};
export type PaymentIntegrationRequest = {
  id: string; country: string; providerName: string; terminalModel: string; capability: string;
  details: string; status: "local_only" | "queued" | "acknowledged" | "closed"; createdAt: string;
};
export type NewPaymentIntegrationRequest = Omit<PaymentIntegrationRequest, "id" | "status" | "createdAt">;
export const paymentApi = {
  options: () => invoke<PaymentOption[]>("get_payment_options"),
  save: (input: PaymentOption) => invoke<void>("save_payment_option", { input }),
  integrationRequests: () => invoke<PaymentIntegrationRequest[]>("list_payment_integration_requests"),
  saveIntegrationRequest: (input: NewPaymentIntegrationRequest) => invoke<PaymentIntegrationRequest>("save_payment_integration_request", { input }),
};
export const defaultPaymentOptions: PaymentOption[] = [
  { id: null, name: "Cash", kind: "cash", provider: "", merchantAccount: "", terminalId: "", instructions: "", referenceRequired: false, enabled: true },
  { id: null, name: "Card", kind: "card", provider: "", merchantAccount: "", terminalId: "", instructions: "Confirm approval on the external bank terminal before recording the sale.", referenceRequired: false, enabled: true },
  { id: null, name: "Mobile money", kind: "mobile_money", provider: "", merchantAccount: "", terminalId: "", instructions: "Confirm receipt in your merchant account before recording the sale.", referenceRequired: false, enabled: true },
];
