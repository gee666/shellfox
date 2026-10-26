import { NativeModules, Platform } from 'react-native';
import { z } from 'zod';
import { createUpdateClient, type UpdatesBridge } from './client';

function nativeModule(): UpdatesBridge {
  const modules: Record<string, unknown> = NativeModules;
  const value = modules.ShellfoxUpdates;
  if (typeof value !== 'object' || value === null ||
      !('getVersion' in value) || typeof value.getVersion !== 'function' ||
      !('install' in value) || typeof value.install !== 'function') {
    throw new Error('ShellfoxUpdates native module is unavailable.');
  }
  const getVersion = value.getVersion.bind(value), install = value.install.bind(value);
  return {
    async getVersion() { const version: unknown = await getVersion(); return z.string().parse(version); },
    async install(url, sha256) { await install(url, sha256); },
  };
}

// Resolve lazily so unsupported platforms can render without an APK module.
export const updates = createUpdateClient({
  platform: Platform.OS,
  fetch: (...args) => fetch(...args),
  bridge: {
    getVersion: () => nativeModule().getVersion(),
    install: (url, sha256) => nativeModule().install(url, sha256),
  },
});
