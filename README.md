# MyKVM Local

MyKVM Local is a security-focused fork of [XxMinor/mykvm](https://github.com/XxMinor/mykvm), based on upstream commit [`a2ea4164861de31b562c8417eeb7879dbc8c23cb`](https://github.com/XxMinor/mykvm/commit/a2ea4164861de31b562c8417eeb7879dbc8c23cb). It remains available under the upstream MIT license.

This fork targets one specific setup: a Windows PC supplies the physical keyboard and mouse, while an Apple Silicon Mac runs Codex, terminals, an IDE, and a browser. Each computer keeps its own directly connected display. MyKVM Local transfers input and optional clipboard data; it does not stream video.

[简体中文](./README.zh-CN.md) · [Upstream project](https://github.com/XxMinor/mykvm) · [Detailed delivery status](./docs/DELIVERY.md)

## Changes from upstream

- Added explicit **Control Mac**, **Return to Windows**, and **Emergency return** actions with configurable hotkeys.
- Added a Windows local-game mode that disables edge switching and exits the Windows hook before layout, cursor, network, or logging work.
- Replaced the legacy data path with a bounded V2 QUIC protocol for control, reliable input, and latest-wins pointer motion.
- Bound inbound data to the certificate presented by the live TLS connection, the persisted paired peer, its role, session, process generation, and sequence number. Discovery cannot silently change trust.
- Added session-owned key/button tracking and release on normal return, End, stream loss, lease expiry, faults, and emergency return.
- Added explicit Mac modifier mapping. Windows Ctrl remains Mac Control by default; Windows keys map to Command, with an optional Ctrl/Command swap preset.
- Added bidirectional, versioned text clipboard sync with exact echo suppression. Image sync is opt-in, pauses in local-game mode, and is protected by format checks and a global bulk-memory budget.
- Moved the runtime out of the settings WebView. The settings window can be destroyed and reopened while the Rust background process continues.
- Added Simplified Chinese settings and menu-bar actions, user-level opt-in autostart, single-instance activation, IPC validation, and redacted diagnostics.
- Isolated the fork identity as `local.mykvm.gaming`; disabled upstream auto-update, privileged helper installation, SYSTEM services, automatic firewall changes, and upstream release automation.
- Added fake-platform tests, authenticated local QUIC loopback tests, native Mac/Windows CI, unsigned preview packaging, checksums, and a passive resource sampler.

## Current status

| Area | Status |
| --- | --- |
| Automated tests | 225 Rust tests and 23 isolation tests pass |
| macOS build | ARM64 app and DMG generated and verified |
| macOS runtime | Not run; no Accessibility/TCC changes were made |
| Windows build | CI and NSIS script ready; native runner has not completed yet |
| Windows physical input | Not run |
| League of Legends | Optional and not run |

The Mac artifact is an unsigned, unnotarized development preview. The source and automated path are ready for controlled testing, but this repository does not yet claim that Windows-to-Mac operation has passed physical two-machine testing.

## Safety and scope

- Input and optional clipboard only. No display capture, video transport, driver, game injection, privileged service, secure-desktop helper, or anti-cheat bypass.
- Legacy LAN input/clipboard/file endpoints fail closed. V2 input is accepted only from a paired controller over an authenticated QUIC connection.
- Text and images have independent limits. Raw images are capped at 32 MiB, encoded bulk frames at 48 MiB, and aggregate bulk working memory at 128 MiB.
- Image clipboard sync defaults off. Autostart is also opt-in.
- The project does not promise zero GPU use, universal game compatibility, or control of Windows secure desktops.

## Build and test

Requirements:

- Node.js 22
- Rust 1.98.1 for the recorded build
- Xcode Command Line Tools on macOS
- Visual Studio 2022 C++ Build Tools and WebView2 on Windows

Run the complete non-interactive checks without launching the desktop app:

```bash
npm ci
node scripts/check-native.mjs
```

Build the unsigned Apple Silicon preview on macOS:

```bash
sh scripts/build-mac-arm.sh
shasum -a 256 -c docs/PREVIEW_ARTIFACTS.sha256
```

Build the unsigned NSIS preview on native Windows:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-preview.ps1
```

The Windows output is written to `src-tauri\target\release\bundle\nsis\`, together with `SHA256SUMS`.

## CI artifacts

`.github/workflows/native-preview.yml` runs non-interactive checks on macOS 14 and Windows Server 2022. The Windows job also builds an unsigned NSIS installer and uploads it as `windows-preview-<commit SHA>` for seven days. The workflow has read-only repository permissions and does not create a GitHub Release.

## Controlled first run

Do not replace an existing upstream installation. Verify the checksum and install this fork under its independent **MyKVM Local** name. The Mac should use the receiver role; the Windows machine should use the controller role. Pair both devices with the six-digit code, confirm the monitor layout and all three control hotkeys, then test on a normal desktop before enabling clipboard images, autostart, or any game scenario.

macOS input injection requires Accessibility permission. This repository does not disable Gatekeeper or modify TCC automatically. See [the delivery guide](./docs/DELIVERY.md) for the current artifact paths, unverified items, and rollback steps.

## Documentation

- [Implementation progress](./docs/PROGRESS.md)
- [Test report](./docs/TEST_REPORT.md)
- [Source and security audit](./docs/SOURCE_AUDIT.md)
- [Task board](./docs/handoff/taskboard.json)
- [Mac preview build evidence](./docs/T22-mac-preview.md)
- [Windows preview pipeline](./docs/T23-windows-preview.md)
- [Resource validation procedure](./docs/T24-resource-validation.md)

## License and attribution

Copyright and attribution from the original project are retained. This fork is derived from [XxMinor/mykvm](https://github.com/XxMinor/mykvm) and is distributed under the [MIT License](./LICENSE). MyKVM Local is an independent fork and is not presented as an official upstream release.
