// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { emit } from "@tauri-apps/api/event";
import { getAllWebviewWindows, WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { availableMonitors, currentMonitor, PhysicalPosition, primaryMonitor, type Monitor } from "@tauri-apps/api/window";
import type { StorePreferences } from "./models";

export type CustomerCartLine = {
  name: string;
  quantity: number;
  unitPriceMinor: number;
  lineTotalMinor: number;
};

export type CustomerDisplayCart = {
  paymentInstructions?: { name: string; provider: string; account: string; instructions: string } | null;
  businessName: string;
  storeName: string;
  currencyCode: string;
  lines: CustomerCartLine[];
  subtotalMinor: number;
  discountMinor: number;
  discountPendingApproval: boolean;
  totalMinor: number;
  status: "idle" | "basket" | "paid";
  receiptNumber: string | null;
};

export type DisplayTarget = {
  key: string;
  label: string;
  x: number;
  y: number;
  width: number;
  height: number;
  scaleFactor: number;
  isPrimary: boolean;
};

function samePosition(a: Monitor | null, b: { x: number; y: number }) {
  return a?.position.x === b.x && a.position.y === b.y;
}

export function displayTargetKey(monitor: Monitor) {
  return `${monitor.name ?? "Display"}:${monitor.position.x}:${monitor.position.y}`;
}

export async function listDisplayTargets(): Promise<DisplayTarget[]> {
  const [monitors, primary] = await Promise.all([availableMonitors(), primaryMonitor()]);
  return monitors.map((monitor, index) => {
    const isPrimary = samePosition(primary, monitor.position);
    const width = Math.round(monitor.size.width / monitor.scaleFactor);
    const height = Math.round(monitor.size.height / monitor.scaleFactor);
    const name = monitor.name?.trim() || `Screen ${index + 1}`;
    return {
      key: displayTargetKey(monitor),
      label: `${isPrimary ? "Main" : `Screen ${index + 1}`} · ${name} · ${width} × ${height}`,
      x: monitor.position.x,
      y: monitor.position.y,
      width,
      height,
      scaleFactor: monitor.scaleFactor,
      isPrimary,
    };
  });
}

export async function openCustomerDisplay(preferences: StorePreferences): Promise<void> {
  const existing = (await getAllWebviewWindows()).find((window) => window.label === "customer-display");
  if (existing) {
    await existing.show();
    return;
  }

  const targets = await listDisplayTargets();
  if (!targets.length) throw new Error("Advanced POS could not find a connected display.");
  const target = targets.find((screen) => screen.key === preferences.customerDisplayMonitorKey)
    ?? targets.find((screen) => !screen.isPrimary)
    ?? targets[0];
  const mainMonitor = await currentMonitor();
  if (preferences.customerDisplayFullscreen && samePosition(mainMonitor, target)) {
    throw new Error("Choose a different screen before full-screening the customer display.");
  }

  const idle = new URLSearchParams({
    screen: "customer-display",
    idle: preferences.customerDisplayIdleMessage,
  });
  const displayWindow = new WebviewWindow("customer-display", {
    url: `index.html?${idle.toString()}`,
    title: "Advanced POS · Customer display",
    x: target.x / target.scaleFactor,
    y: target.y / target.scaleFactor,
    width: target.width,
    height: target.height,
    focus: false,
    resizable: true,
  });

  await new Promise<void>((resolve, reject) => {
    displayWindow.once("tauri://created", () => {
      void (async () => {
        if (preferences.customerDisplayFullscreen) {
          await displayWindow.setFullscreenOnMonitor(new PhysicalPosition(target.x, target.y));
        }
        resolve();
      })().catch(reject);
    });
    displayWindow.once("tauri://error", (event) => reject(new Error(String(event.payload))));
  });
}

export function publishCustomerDisplayCart(cart: CustomerDisplayCart) {
  return emit("nimbletill:customer-display-cart", cart);
}
