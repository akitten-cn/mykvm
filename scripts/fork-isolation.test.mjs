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

test('T04.b3: incomplete Windows controller stays compile-time closed', () => {
  assert.match(read('src-tauri/src/fork_policy.rs'), /V2_NATIVE_CONTROLLER_ENABLED: bool = false/)
  const lib = read('src-tauri/src/lib.rs')
  assert.match(lib, /V2_NATIVE_CONTROLLER_ENABLED[\s\S]*?controller_mode_enabled/)
  const input = read('src-tauri/src/input.rs')
  assert.match(input, /controller\.poll\([\s\S]*?windows_control_inputs_released\(\),[\s\S]*?&mut capture,[\s\S]*?&mut focus/)
  assert.match(input, /fn windows_control_inputs_released\(\)[\s\S]*?GetAsyncKeyState/)
  assert.match(input, /impl FocusPort for WindowsV2FocusPort[\s\S]*?Err\(PortError::Unavailable\)/)
  assert.match(input, /fn send_v2_windows_motion[\s\S]*?controller\.send_motion/)
  assert.match(input, /v2_motion_sequence[\s\S]*?store\(sequence, Ordering::Release\)/)
})

test('A08/A28: Windows local game hooks bypass context and contain panics', () => {
  const gameMode = read('src-tauri/src/game_mode.rs')
  assert.match(gameMode, /static LOCAL_GAME_MODE: AtomicBool/)
  const input = read('src-tauri/src/input.rs')
  assert.match(input, /fn windows_mouse_proc\([\s\S]*?local_game_mode_enabled\(\)[\s\S]*?CallNextHookEx[\s\S]*?catch_unwind[\s\S]*?windows_mouse_proc_inner/)
  assert.match(input, /fn windows_keyboard_proc\([\s\S]*?local_game_mode_enabled\(\)[\s\S]*?CallNextHookEx[\s\S]*?catch_unwind[\s\S]*?windows_keyboard_proc_inner/)
  assert.match(input, /Err\(_\)[\s\S]*?local_override\.request_local\(\)[\s\S]*?CallNextHookEx/)
})
