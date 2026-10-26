import { TurboModuleRegistry, type TurboModule } from 'react-native';

interface TerminalKeyboardModule extends TurboModule {
  show(): Promise<void>;
}
export async function showTerminalKeyboard(): Promise<void> {
  const keyboard = TurboModuleRegistry.get<TerminalKeyboardModule>('ShellfoxTerminalKeyboard');
  if (!keyboard) throw new Error('Terminal keyboard is unavailable.');
  await keyboard.show();
}
