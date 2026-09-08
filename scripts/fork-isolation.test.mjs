import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, readdirSync } from 'node:fs'

const read = path => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8')
const json = path => JSON.parse(read(path))

test('A42: fork has independent app/data identity and no update artifacts', () => {
  const config = json('src-tauri/tauri.conf.json')
  assert.equal(config.identifier, 'local.mykvm.gaming')
  assert.equal(config.productName, 'MyKVM Local')
  assert.equal(config.bundle.createUpdaterArtifacts, false)
  assert.equal(config.plugins?.updater, undefined)
  assert.equal(config.bundle.windows?.nsis?.installerHooks, undefined)
  assert.equal(json('src-tauri/tauri.windows.conf.json').bundle.externalBin, undefined)
})

test('A42: updater is disabled at registration, capability, API and UI', () => {
  assert.doesNotMatch(read('src-tauri/src/lib.rs'), /plugin\(tauri_plugin_updater/)
  assert.ok(!json('src-tauri/capabilities/default.json').permissions.some(p => p.startsWith('updater:')))
  assert.doesNotMatch(read('src/desktopApi.ts'), /import\(['"]@tauri-apps\/plugin-updater/)
  assert.match(read('src/constants.ts'), /UPDATES_ENABLED = false/)
  assert.match(read('src/App.tsx'), /UPDATES_ENABLED && <section/)
})

test('A40: packaging does not install helpers, alter trust or copy installed apps', () => {
  for (const path of ['scripts/build-tauri-assets.mjs', 'scripts/build-mac-arm.sh',
    'scripts/install-mac-app.sh', 'scripts/sign-mac-app.sh', 'src-tauri/build.rs']) {
    assert.doesNotMatch(read(path), /security add|delete-keychain|xattr -|pkill|osascript|ditto|input-helper|sudo/)
  }
  const scripts = json('package.json').scripts
  assert.equal(scripts['mac:build-install'], undefined)
  assert.equal(scripts['mac:sign-local'], undefined)
  assert.equal(scripts['mac:install-local'], undefined)
  assert.equal(scripts['tauri:build:mac-signed'], undefined)
})

test('A40: privileged routes fail closed, even when called outside the UI', () => {
  const source = read('src-tauri/src/lib.rs')
  for (const name of ['restart_as_admin', 'install_input_service', 'uninstall_input_service', 'send_secure_attention']) {
    assert.match(source, new RegExp(`fn ${name}\\([\\s\\S]*?\\{\\s*crate::fork_policy::require_privileged_features\\(\\)\\?;`))
  }
  assert.match(source, /if !crate::fork_policy::PRIVILEGED_FEATURES_ENABLED/)
  assert.match(read('src-tauri/src/input.rs'), /if !crate::fork_policy::PRIVILEGED_FEATURES_ENABLED/)
  assert.match(read('src-tauri/src/fork_policy.rs'), /PRIVILEGED_FEATURES_ENABLED: bool = false/)
  assert.match(source, /fn ensure_windows_firewall_rule\(\) \{\s*if !crate::fork_policy::PRIVILEGED_FEATURES_ENABLED \{\s*return;/)
})

test('A42: single-instance, helper namespace and frontend storage are isolated', () => {
  assert.doesNotMatch(read('src-tauri/src/lib.rs'), /Local\\\\MyKVM_(?:SingleInstance|ActivateWindow|QuitExisting)/)
  assert.doesNotMatch(read('src-tauri/src/shared_input.rs'), /"MyKVMInputService"|pipe\\mykvm-input-s/)
  assert.doesNotMatch(read('src/App.tsx'), /"mykvm:/)
  assert.match(read('src-tauri/src/main.rs'), /windows_subsystem = "windows"/)
})

test('A39-A41: background runtime owns lifecycle and autostart stays silent', () => {
  const source = read('src-tauri/src/lib.rs')
  const ui = read('src/App.tsx')
  assert.match(source, /let runtime = AppRuntime::new\([\s\S]*?app\.manage\(runtime\)/)
  assert.match(source, /fn destroy_main_window_handle[\s\S]*?window\s*\.destroy\(\)/)
  assert.match(source, /WindowEvent::CloseRequested[\s\S]*?api\.prevent_close\(\)[\s\S]*?hide_main_window_handle/)
  assert.match(source, /let silent_launch = launched_from_autostart\(\)[\s\S]*?if silent_launch \{\s*hide_main_window_handle/)
  assert.match(source, /MacosLauncher::LaunchAgent/)
  assert.doesNotMatch(ui, /mykvm\.clientAutostartInit/)
  assert.match(ui, /unlistenRuntime\?\.\(\)/)
  assert.match(ui, /window\.clearInterval\(intervalId\)/)
  assert.match(source, /UNIX_INSTANCE_SOCKET_NAME/)
  assert.match(source, /bind_unix_instance[\s\S]*?UnixInstanceBind::Existing/)
  assert.doesNotMatch(source, /PreventSystemSleep|SetThreadExecutionState|SYSTEM\\CurrentControlSet\\Services/)
  assert.match(read('src-tauri/tauri.conf.json'), /"windows": \[\]/)
  assert.match(source, /fn ensure_main_window[\s\S]*?WebviewWindowBuilder::new/)
})

test('A42: fork cannot automatically run the upstream release workflow', () => {
  for (const file of readdirSync(new URL('../.github/workflows/', import.meta.url))) {
    assert.doesNotMatch(read(`.github/workflows/${file}`), /TAURI_SIGNING_PRIVATE_KEY|contents: write|gh release/)
  }
})

test('T05.a: legacy LAN paths cannot activate before V2 authentication exists', () => {
  assert.match(read('src-tauri/src/fork_policy.rs'), /LEGACY_LAN_DATA_ENABLED: bool = false/)
  const lib = read('src-tauri/src/lib.rs')
  for (const name of ['start_discovery', 'start_input', 'start_clipboard', 'handle_clipboard_packet',
    'handle_file_transfer_packet', 'send_files_to_device']) {
    assert.match(lib, new RegExp(`fn ${name}\\([\\s\\S]*?\\{\\s*if !crate::fork_policy::LEGACY_LAN_DATA_ENABLED`))
  }
  const input = read('src-tauri/src/input.rs')
  for (const name of ['handle_input_datagram', 'input_receive_status', 'input_runtime_status']) {
    assert.match(input, new RegExp(`fn ${name}\\([\\s\\S]*?\\{\\s*if !crate::fork_policy::LEGACY_LAN_DATA_ENABLED`))
  }
})

test('T07: native input is enabled only on the authenticated V2 path', () => {
  assert.match(read('src-tauri/src/fork_policy.rs'), /V2_NATIVE_RECEIVER_ENABLED: bool = true/)
  const lib = read('src-tauri/src/lib.rs')
  assert.match(lib, /V2_NATIVE_RECEIVER_ENABLED[\s\S]*?receiver_mode_enabled/)
})

test('T04.b3: reviewed Windows V2 controller is the only enabled control path', () => {
  assert.match(read('src-tauri/src/fork_policy.rs'), /V2_NATIVE_CONTROLLER_ENABLED: bool = true/)
  const lib = read('src-tauri/src/lib.rs')
  assert.match(lib, /V2_NATIVE_CONTROLLER_ENABLED[\s\S]*?controller_mode_enabled/)
  const input = read('src-tauri/src/input.rs')
  assert.match(input, /controller\.poll\([\s\S]*?windows_control_inputs_released\(\),[\s\S]*?&mut capture,[\s\S]*?&mut focus/)
  assert.match(input, /fn windows_control_inputs_released\(\)[\s\S]*?GetAsyncKeyState/)
  assert.match(input, /impl FocusPort for WindowsV2FocusPort[\s\S]*?prepare_windows_focus\(self\.context\)/)
  assert.match(input, /fn send_v2_windows_motion[\s\S]*?controller\.send_motion/)
  assert.match(input, /v2_motion_sequence[\s\S]*?store\(sequence, Ordering::Release\)/)
})

test('A09: Windows focus handoff is one-shot and reports conservative fallback', () => {
  const input = read('src-tauri/src/input.rs')
  const body = input.match(/fn prepare_windows_focus\([\s\S]*?\n\}/)?.[0]
  assert.ok(body, 'focus adapter is present')
  assert.equal((body.match(/SetForegroundWindow/g) ?? []).length, 2)
  assert.doesNotMatch(body, /loop\s*\{|while\s|sleep\(|SendInput|keybd_event/)
  assert.match(body, /Alt\+Tab/)
  assert.match(input, /fn restore_windows_foreground\([\s\S]*?previous_foreground\.swap\([\s\S]*?SetForegroundWindow/)
})

test('A29: V2 production motion is bound to display layout and mapped before injection', () => {
  const protocol = read('src-tauri/src/protocol_v2.rs')
  assert.match(protocol, /pub struct MotionFrame \{[\s\S]*?display_id: String,[\s\S]*?layout_revision: u64/)
  assert.match(protocol, /ControlFrame::Prepare \{[\s\S]*?target_display,[\s\S]*?layout_revision/)
  const session = read('src-tauri/src/session_runtime.rs')
  assert.match(session, /frame\.display_id != active_display\.display_id[\s\S]*?frame\.layout_revision != active_display\.layout_revision/)
  assert.match(session, /fn map_active_pointer[\s\S]*?\.map\(x, y\)/)
  assert.match(session, /update_display_layouts[\s\S]*?SessionFault::LayoutChanged[\s\S]*?release_pressed/)
  const input = read('src-tauri/src/input.rs')
  assert.match(input, /ControllerTarget \{[\s\S]*?target_display:[\s\S]*?layout_revision:/)
})

test('A30/A31: Mac modifier policy is explicit and receiver mapping freezes on key-down', () => {
  const backend = read('src-tauri/src/lib.rs')
  assert.match(backend, /fn default_modifier_remap\(\) -> bool \{\s*false\s*\}/)
  for (const name of ['control', 'alt', 'meta']) {
    assert.match(backend, new RegExp(`fn default_modifier_${name}\\(\\) -> String \\{\\s*"same"\\.into\\(\\)\\s*\\}`))
  }
  assert.match(read('src/defaultLayout.ts'), /modifierRemap: false,[\s\S]*?modifierMap: \{ control: 'same', alt: 'same', meta: 'same' \}/)

  const session = read('src-tauri/src/session_runtime.rs')
  assert.match(session, /let mut mapped_event = frame\.event\.clone\(\);[\s\S]*?remap_modifier_vk\([\s\S]*?self\.pressed\.apply\(&mapped_event/)
  const input = read('src-tauri/src/input.rs')
  assert.doesNotMatch(input, /macos_post_select_previous_input_source|MACOS_CAPS_LOCK_DOWN/)
  assert.match(input, /\(57, 0x14\)/)

  assert.match(backend, /receiver\.update_modifier_mapping\([\s\S]*?receiver[\s\S]*?\.handle_input_at\(/)
  assert.match(backend, /if !crate::fork_policy::LEGACY_LAN_DATA_ENABLED[\s\S]*?if controller_enabled[\s\S]*?return statuses;[\s\S]*?input::v2_inject_status\(\)/)
})

test('T19: safe loopback is test-only and cannot reach native desktop adapters', () => {
  const backend = read('src-tauri/src/lib.rs')
  assert.match(backend, /#\[cfg\(test\)\]\s*mod safe_loopback;/)
  const loopback = read('src-tauri/src/safe_loopback.rs')
  assert.match(loopback, /FakeInjector::default\(\)/)
  assert.match(loopback, /receiver-identity/)
  assert.match(loopback, /controller-identity/)
  assert.doesNotMatch(loopback, /NativeInjector|start_input_runtime|start_v2_controller_runtime|start_capture|CGEvent|SendInput/)
})

test('T13: Windows clipboard uses a user-session listener with symmetric cleanup', () => {
  const clipboard = read('src-tauri/src/clipboard.rs')
  assert.match(clipboard, /AddClipboardFormatListener\(window\)/)
  assert.match(clipboard, /WM_CLIPBOARDUPDATE[\s\S]*?GetClipboardSequenceNumber/)
  assert.match(clipboard, /WM_CLOSE[\s\S]*?RemoveClipboardFormatListener\(window\)[\s\S]*?DestroyWindow\(window\)/)
  assert.match(clipboard, /WM_NCDESTROY[\s\S]*?Box::from_raw\(context\)[\s\S]*?PostQuitMessage/)
  assert.match(clipboard, /ClipboardRead::Busy[\s\S]*?Duration::from_millis\(4\)/)
  assert.doesNotMatch(clipboard, /OpenClipboard[\s\S]*?OpenProcess|WTSGetActiveConsoleSessionId|SYSTEM/)
  const backend = read('src-tauri/src/lib.rs')
  assert.match(backend, /WindowsClipboardListener::start\(\)/)
  assert.match(backend, /windows_listener\.wait_for_change/)
})

test('A37: Mac clipboard stays in-process and checks changeCount before reading', () => {
  const clipboard = read('src-tauri/src/clipboard.rs')
  assert.match(clipboard, /NSPasteboard::generalPasteboard\(\)\.changeCount\(\)/)
  assert.match(clipboard, /struct MacClipboardWatcher[\s\S]*?wait_for_change/)
  assert.doesNotMatch(clipboard, /pbpaste|pbcopy/)
  const backend = read('src-tauri/src/lib.rs')
  assert.match(backend, /MacClipboardWatcher::start\(\)/)
  assert.match(backend, /mac_watcher\.wait_for_change[\s\S]*?read_content_typed\(\)/)
})

test('A32-A36: V2 clipboard is authenticated, versioned, bounded, and user-controlled', () => {
  const protocol = read('src-tauri/src/protocol_v2.rs')
  assert.match(protocol, /struct ClipboardTextOperation[\s\S]*?operation_id:[\s\S]*?origin_peer:[\s\S]*?system_revision:[\s\S]*?lamport:[\s\S]*?digest:/)
  assert.match(protocol, /clipboard_text_digest[\s\S]*?SHA256/)
  const sync = read('src-tauri/src/clipboard_sync.rs')
  assert.match(sync, /applied_remote_echo[\s\S]*?system_revision[\s\S]*?expected == digest/)
  assert.match(sync, /IgnoreDuplicate[\s\S]*?IgnoreStale/)
  const transport = read('src-tauri/src/quic_transport.rs')
  assert.match(transport, /fn trusted_bulk_peer[\s\S]*?trust_store[\s\S]*?control_peer/)
  const backend = read('src-tauri/src/lib.rs')
  assert.match(backend, /origin_peer != &authenticated\.peer_id/)
  assert.match(backend, /trusted_bulk_peer\(/)
  assert.match(backend, /manual_resend_text/)
  const ui = read('src/App.tsx')
  assert.match(ui, /clipboardTextLimitBytes/)
  assert.match(ui, /resendCurrentClipboard/)
})

test('A27/A38: image clipboard is opt-in and bulk memory is bounded before decode', () => {
  assert.match(read('src/defaultLayout.ts'), /clipboardImageSync: false/)
  const backend = read('src-tauri/src/lib.rs')
  assert.match(backend, /clipboard_image_sync && !layout\.game_mode/)
  assert.match(backend, /validate_image_(encoding|data)/)
  const clipboard = read('src-tauri/src/clipboard.rs')
  const writeImage = clipboard.match(/fn write_image\([\s\S]*?\n\}/)?.[0]
  assert.ok(writeImage, 'image writer is present')
  assert.ok(writeImage.indexOf('validate_image_encoding') < writeImage.indexOf('.decode('))
  assert.match(clipboard, /checked_mul\(height\)[\s\S]*?checked_mul\(4\)/)
  const transport = read('src-tauri/src/quic_transport.rs')
  assert.match(transport, /MAX_BULK_MEMORY_BYTES: usize = 128 \* 1024 \* 1024/)
  assert.match(transport, /bulk_memory\.reserve\(INBOUND_BULK_RESERVATION_BYTES\)[\s\S]*?read_to_end/)
})

test('A08/A28: Windows local game hooks bypass context and contain panics', () => {
  const gameMode = read('src-tauri/src/game_mode.rs')
  assert.match(gameMode, /static LOCAL_GAME_MODE: AtomicBool/)
  const input = read('src-tauri/src/input.rs')
  assert.match(input, /fn windows_capture_context\(\)[\s\S]*?WINDOWS_CAPTURE_CONTEXT[\s\S]*?\.try_lock\(\)/)
  assert.match(input, /fn windows_mouse_proc\([\s\S]*?local_game_mode_enabled\(\)[\s\S]*?CallNextHookEx[\s\S]*?catch_unwind[\s\S]*?windows_mouse_proc_inner/)
  assert.match(input, /fn windows_keyboard_proc\([\s\S]*?local_game_mode_enabled\(\)[\s\S]*?CallNextHookEx[\s\S]*?catch_unwind[\s\S]*?windows_keyboard_proc_inner/)
  assert.match(input, /Err\(_\)[\s\S]*?local_override\.request_local\(\)[\s\S]*?CallNextHookEx/)
  for (const name of ['windows_mouse_proc_inner', 'windows_keyboard_proc_inner']) {
    const body = input.match(new RegExp(`unsafe fn ${name}\\([\\s\\S]*?\\n\\}`))?.[0]
    assert.ok(body, `${name} body is present`)
    assert.doesNotMatch(body, /\.lock\(\)|handle_windows_|send_v2_|send_packet|set_windows_cursor|ShowCursor/)
    assert.match(body, /try_offer_hook_event\([\s\S]*?WindowsHookEvent/)
  }
})
