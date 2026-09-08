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
