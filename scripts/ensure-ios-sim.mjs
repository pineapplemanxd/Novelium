#!/usr/bin/env node
import { execFileSync, spawnSync } from "node:child_process";

function ensureIosSimulatorBooted() {
  if (process.platform !== "darwin") {
    return;
  }

  try {
    const raw = execFileSync("xcrun", ["simctl", "list", "devices", "available", "-j"], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    });
    const parsed = JSON.parse(raw);
    const devicesByRuntime = parsed?.devices ?? {};

    const iosDevices = [];
    for (const [runtime, list] of Object.entries(devicesByRuntime)) {
      if (!runtime.includes("iOS") || !Array.isArray(list)) continue;
      for (const dev of list) {
        if (dev && dev.isAvailable !== false && dev.udid) {
          iosDevices.push(dev);
        }
      }
    }

    if (iosDevices.length === 0) {
      return;
    }

    const requested =
      process.env.IOS_SIMULATOR?.trim() ||
      process.env.TARGET_DEVICE_IDENTIFIER?.trim();
    const target =
      (requested &&
        iosDevices.find(
          (d) =>
            d.udid.toLowerCase() === requested.toLowerCase() ||
            d.name.toLowerCase() === requested.toLowerCase(),
        )) ||
      iosDevices.find((d) => d.state === "Booted") ||
      iosDevices[0];

    // Also ensure the default first simulator (which `tauri ios dev` picks when no
    // device name is passed) is booted so `xcrun simctl install <udid>` never fails
    // with exit code 149 (Unable to lookup in current state: Shutdown).
    const toBoot = [target];
    if (iosDevices[0] && iosDevices[0].udid !== target.udid) {
      toBoot.push(iosDevices[0]);
    }

    for (const dev of toBoot) {
      if (dev.state !== "Booted") {
        console.log(`[ios-sim] Booting iOS Simulator: ${dev.name} (${dev.udid})...`);
        spawnSync("xcrun", ["simctl", "boot", dev.udid], { stdio: "inherit" });
        spawnSync("xcrun", ["simctl", "bootstatus", dev.udid, "-b"], {
          stdio: "inherit",
        });
      } else {
        console.log(`[ios-sim] iOS Simulator already booted: ${dev.name} (${dev.udid})`);
      }
    }

    spawnSync("open", ["-a", "Simulator", "--args", "-CurrentDeviceUDID", target.udid], {
      stdio: "ignore",
    });
  } catch (err) {
    console.warn("[ios-sim] Could not pre-boot iOS Simulator:", err?.message ?? err);
  }
}

ensureIosSimulatorBooted();
