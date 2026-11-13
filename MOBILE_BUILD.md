# Novelium iOS & iPadOS Build Guide

This guide covers building, signing, running, and troubleshooting the native **iOS (iPhone)** and **iPadOS (iPad)** ports of Novelium using **Tauri 2** and **Xcode**.

---

## 1. Prerequisites

- **macOS** with **Xcode 16.4 (`16F6`)** or newer and Xcode Command Line Tools installed.
- **Rust** toolchain with iOS targets installed:
  ```bash
  rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
  ```
- **Homebrew tools**:
  ```bash
  brew install xcodegen libimobiledevice cocoapods
  ```
- **Node.js** dependencies installed (`npm install` at the repository root).
- **iOS Simulator Platform Runtime** (required by `npm run tauri ios dev` and `npm run tauri ios build`):
  ```bash
  xcodebuild -downloadPlatform iOS
  ```
  Alternatively, open **Xcode -> Settings -> Components (Platforms)** and download the **iOS 18.5** runtime.

---

## 2. SDK & Deployment Target Matrix

| Setting | Value | Source |
|---|---|---|
| Host Xcode SDKs verified | `iOS 18.5` (`iphoneos18.5`), `Simulator - iOS 18.5` (`iphonesimulator18.5`) | `xcodebuild -showsdks` |
| Minimum Deployment Target | `15.0` (`IPHONEOS_DEPLOYMENT_TARGET: "15.0"`) | `src-tauri/gen/apple/project.yml`, `src-tauri/tauri.ios.conf.json` |
| Device Family | Universal iPhone & iPad (`TARGETED_DEVICE_FAMILY: "1,2"`) | `src-tauri/gen/apple/project.yml` |
| iPhone Orientations | Portrait, Landscape Left, Landscape Right | `src-tauri/gen/apple/novelium_iOS/Info.plist` |
| iPad Orientations | All 4 orientations (including Upside Down) | `src-tauri/gen/apple/novelium_iOS/Info.plist` |
| Document Types (`CFBundleDocumentTypes`) | EPUB (`org.idpf.epub-container`), PDF (`com.adobe.pdf`), Markdown (`net.daringfireball.markdown`), TXT (`public.plain-text`), HTML (`public.html`), JSON (`public.json`) | `src-tauri/gen/apple/novelium_iOS/Info.plist` |
| Files App Integration | `UIFileSharingEnabled: true`, `LSSupportsOpeningDocumentsInPlace: true` | `src-tauri/gen/apple/novelium_iOS/Info.plist` |
| Network Security | `NSAppTransportSecurity` with `NSAllowsLocalNetworking: true` (HTTP scraping uses Rust `reqwest` + `rustls`) | `src-tauri/gen/apple/novelium_iOS/Info.plist` |
| App Identifier | `com.novelium.reader` (change if needed for Personal Team signing) | `src-tauri/tauri.conf.json` |

### Note on Requested vs. Available SDKs

The installed **Xcode 16.4** provides **`iOS 18.5`** (`iphoneos18.5` / `iphonesimulator18.5`) alongside the **`iOS 18.6` (`22G86`)** Simulator runtime. Requested future **`iOS 27`** and **`iPadOS 26`** SDKs do not exist in Xcode 16.4, so the project is configured for the installed iOS 18 toolchain with deployment compatibility back to iOS/iPadOS `15.0`.

### Verified vs. Remaining Interactive Checks

- Verified offline Rust cross-compilation (`aarch64-apple-ios`, `aarch64-apple-ios-sim`), Xcode project generation (`xcodegen generate`), and all desktop Rust/Vitest suites.
- Detected **`iOS 18.6`** simulator devices in the interactive user session (`iPhone 16 Pro`, `iPad Pro 11-inch (M4)`, `iPad (A16)`, etc.).
- Note: Running `xcodebuild` / `npm run tauri ios build` / `npm run tauri ios dev` requires running inside your logged-in macOS GUI user session (non-GUI root shells report `Found no destinations for the scheme 'novelium_iOS'`). Runtime UI verification inside Simulator and on physical devices remains interactive.

---

## 3. Free Personal Team Signing (No Paid Developer Account)

You do **not** need a paid Apple Developer membership ($99/year) to run Novelium on your personal iPhone or iPad:

1. Add your Apple ID in **Xcode -> Settings -> Accounts** (it appears as a **Personal Team**).
2. Open the generated iOS Xcode project:
   ```bash
   open src-tauri/gen/apple/novelium.xcodeproj
   ```
   (Or let Tauri regenerate assets cleanly: `npm run tauri ios init -- --skip-targets-install`, though note customizations live in `src-tauri/gen/apple/project.yml`.)
3. Select the **novelium_iOS** target -> **Signing & Capabilities**:
   - Check **Automatically manage signing**.
   - Select your **Personal Team** under **Team**.
   - If Apple rejects `com.novelium.reader` as already registered to another team, set a unique **Bundle Identifier** (for example `com.yourname.novelium.reader`) in Xcode and optionally in `src-tauri/tauri.conf.json` (`identifier`).
4. Prepare your physical iPhone or iPad:
   - Connect via USB (after initial pairing, Xcode supports wireless debugging) and tap **Trust This Computer**.
   - Enable **Settings -> Privacy & Security -> Developer Mode** on the device (requires reboot).
   - After the first install from Xcode or `tauri ios dev`, open **Settings -> General -> VPN & Device Management**, select your Apple ID profile, and tap **Trust**.

### Free Account Constraints
- Development provisioning profiles expire after **7 days** (re-building/re-running from Xcode or `npm run tauri ios dev -- --device` refreshes the signature while keeping your SQLite library and downloaded chapters intact, provided the Bundle Identifier and Team remain the same).
- Apple limits free Personal Team accounts to **3 active sideloaded apps** per device.
- Paid-only entitlements (Push Notifications, iCloud) are omitted and not used by Novelium.

---

## 4. Common Commands

### Download Simulator Runtime (One-Time Setup)
```bash
xcodebuild -downloadPlatform iOS
```

### Run Live Development Build
```bash
# Launch on iPhone 18.6 Simulator
npm run tauri ios dev "iPhone 16 Pro"

# Launch on iPad 18.6 Simulator
npm run tauri ios dev "iPad Pro 11-inch (M4)"

# Launch on connected iPhone or iPad
npm run tauri ios dev -- --device
```

### Build Release & IPA
```bash
# Release archive + IPA export (uses src-tauri/gen/apple/ExportOptions.plist)
npm run tauri ios build

# Release build targeting Simulator architecture
npm run tauri ios build -t aarch64-sim
```

### Regenerate Xcode Project After Editing `project.yml`
```bash
cd src-tauri/gen/apple && xcodegen generate
```

### Offline Rust Cross-Compilation Checks (Work Without Simulator Runtime)
```bash
cargo check --manifest-path src-tauri/Cargo.toml --target aarch64-apple-ios
cargo check --manifest-path src-tauri/Cargo.toml --target aarch64-apple-ios-sim
```

### macOS Desktop Release Builds
```bash
# Build macOS .app bundle
npm run tauri -- build --bundles app --ci

# Build macOS .dmg installer (note: Tauri's dmg bundler cleans bundle/macos/Novelium.app afterward,
# so run --bundles app separately afterwards if you want both artifacts on disk)
npm run tauri -- build --bundles dmg --ci
```

---

## 5. iOS Storage, Container Updates & Troubleshooting

- **Persistent Storage**: `novelium.db`, cached covers, imported EPUB/PDF/FB2/MD/TXT/HTML files, and downloaded offline chapters live in the iOS app's **Application Support** directory (`app_data_dir()`). Disposable parsed book caches live in **Library/Caches** (`app_cache_dir()`).
- **App Updates & Sandbox UUID Changes**: iOS assigns a new sandbox container UUID (`.../Application/<UUID>/...`) whenever an app updates or is re-signed over an existing install. Novelium automatically re-anchors stored cover/chapter paths across container UUID changes via `relocate_library_path` and `resolve_stored_path` (`src-tauri/src/downloads/mod.rs`), and pins iOS `download_dir` settings to Application Support even if a desktop JSON backup is imported.
- **Background Suspension**: Active download tasks are automatically paused when the app enters the background (`visibilitychange` / `pagehide` -> `pause_active_downloads`) and cleanly reconciled on launch.
- **`iOS platform not installed` Error**: Download the iOS Simulator runtime with `xcodebuild -downloadPlatform iOS`.
- **`xcrun simctl install <UDID>` exited with code 149 (`Unable to lookup in current state: Shutdown`)**: Exit code `149` (`CoreSimulator.SimError` `405`) occurs when Tauri's CLI attempts to install the built `.app` onto an iOS Simulator (such as `238F643F-2C34-4325-BBB0-6DD4BF050B2C`) that has not booted yet or while `Simulator.app` was already open on a different device. `src-tauri/tauri.ios.conf.json` now runs `scripts/ensure-ios-sim.mjs` via `beforeDevCommand` (`npm run dev:ios`) to automatically run `xcrun simctl boot <UDID>` + `xcrun simctl bootstatus <UDID> -b` before `tauri ios dev` installs the bundle.
- **`cli-options-server.json` Missing When Running `xcodebuild` Manually**: Always build via `npm run tauri ios dev` / `npm run tauri ios build` (or open Xcode via `npm run tauri ios build -- --open` / `npm run tauri ios dev -- --open`), which runs Tauri's `xcode-script` asset/Rust coordinator.
