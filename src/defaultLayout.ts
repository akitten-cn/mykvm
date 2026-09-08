import type { LayoutState } from './types'

const platformName = navigator.platform.toLowerCase()
const browserPlatform = platformName.includes('mac')
  ? 'macos'
  : platformName.includes('win')
    ? 'windows'
    : 'unknown'
const browserScreen = window.screen as typeof window.screen & {
  availLeft?: number
  availTop?: number
}
const browserWidth = Math.max(window.screen.width, 1)
const browserHeight = Math.max(window.screen.height, 1)

export const defaultLayout: LayoutState = {
  activeDeviceId: 'local-device',
  selectedScreenId: 'browser-display-1',
  inputMode: 'control',
  machineRole: 'unset',
  clusterId: 'browser-cluster',
  pairSecret: 'browser-secret',
  pairedControllers: [],
  clipboardSync: false,
  clipboardTextLimitBytes: 1024 * 1024,
  fileTransferEnabled: true,
  language: 'cn',
  themeMode: 'system',
  performanceMonitor: false,
  transportPortMode: 'auto',
  transportPort: 47833,
  quicPort: 47834,
  modifierRemap: false,
  modifierMap: { control: 'same', alt: 'same', meta: 'same' },
  edgeSwitchHotkey: 'alt+shift+k',
  screenSwitchHotkeys: { left: 'alt+left', right: 'alt+right', up: 'alt+up', down: 'alt+down' },
  controlHotkeys: {
    controlMac: 'control+alt+F11',
    returnWindows: 'control+alt+F10',
    emergencyReturn: 'control+alt+shift+F10',
  },
  gameMode: false,
  devices: [
    {
      id: 'local-device',
      name: 'Desktop fallback',
      platform: browserPlatform,
      host: window.location.hostname || 'localhost',
      transportPort: 47833,
      quicPort: 47834,
      transportPublicKey: '',
      protocolVersion: 1,
      color: '#2f7af8',
      online: true,
      inputReady: false,
      role: 'local',
      source: 'detected',
      screens: [
        {
          id: 'browser-display-1',
          deviceId: 'local-device',
          name: 'Browser display',
          x: browserScreen.availLeft ?? 0,
          y: browserScreen.availTop ?? 0,
          width: Math.round(browserWidth),
          height: Math.round(browserHeight),
          scale: window.devicePixelRatio || 1,
          isPrimary: true,
        },
      ],
    },
  ],
}
