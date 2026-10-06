// Bundler alias exists only in the explicitly selected test build.
declare module '#test-native-backend' {
  export function createFakeNativeBackend(): import('../shared/native-port').NativeBackend;
}
