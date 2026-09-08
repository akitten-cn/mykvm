// Non-interactive checks only: never launches MyKVM or a real input backend.
import { mkdirSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawnSync } from 'node:child_process'

const root = dirname(dirname(fileURLToPath(import.meta.url)))
const output = resolve(root, '.local-evidence/native')
mkdirSync(output, { recursive: true })
const checks = [
  ['node-version', 'node', ['--version']],
  ['rust-version', 'rustc', ['-vV']],
  ['isolation', 'node', ['--test', 'scripts/fork-isolation.test.mjs']],
  ['lint', 'npm', ['run', 'lint']],
  ['web-build', 'npm', ['run', 'build']],
  ['core-format', 'rustfmt', ['--edition', '2021', '--check',
    'src-tauri/src/control_ports.rs', 'src-tauri/src/routing.rs', 'src-tauri/src/fork_policy.rs']],
  ['lib-check', 'cargo', ['check', '--manifest-path', 'src-tauri/Cargo.toml', '--locked', '--lib']],
  ['lib-tests', 'cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--locked', '--lib']],
]
const results = []
const sha = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).stdout?.trim()
for (const [name, command, args] of checks) {
  const startedAt = new Date().toISOString()
  const windowsNpm = process.platform === 'win32' && command === 'npm'
  const result = spawnSync(windowsNpm ? 'cmd.exe' : command,
    windowsNpm ? ['/d', '/s', '/c', `npm ${args.join(' ')}`] : args,
    { cwd: root, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 })
  const exitCode = result.status ?? 1
  writeFileSync(resolve(output, `${name}.log`),
    `${result.stdout ?? ''}${result.stderr ?? ''}${result.error?.message ?? ''}`)
  results.push({ name, command, args, exitCode, startedAt, endedAt: new Date().toISOString() })
  writeFileSync(resolve(output, 'results.json'), JSON.stringify({ sha,
    platform: process.platform, arch: process.arch, results }, null, 2))
  console.log(`${name}: ${exitCode === 0 ? 'pass' : 'FAIL'} (exit ${exitCode})`)
  if (exitCode !== 0) {
    console.error(`See .local-evidence/native/${name}.log`)
    process.exit(exitCode)
  }
}
