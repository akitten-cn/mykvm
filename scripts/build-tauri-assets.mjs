import { dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawnSync } from 'node:child_process'

const root = dirname(dirname(fileURLToPath(import.meta.url)))

function run(command, args) {
  const resolvedCommand =
    process.platform === 'win32' && command === 'npm'
      ? 'cmd.exe'
      : process.platform === 'win32' && command === 'cargo'
        ? 'cargo.exe'
        : command
  const resolvedArgs =
    process.platform === 'win32' && command === 'npm'
      ? ['/d', '/s', '/c', ['npm', ...args].join(' ')]
      : args
  const result = spawnSync(resolvedCommand, resolvedArgs, {
    cwd: root,
    stdio: 'inherit',
  })

  if (result.error) {
    console.error(result.error.message)
    process.exit(1)
  }

  if (result.status !== 0) {
    process.exit(result.status ?? 1)
  }
}

run('npm', ['run', 'build'])
