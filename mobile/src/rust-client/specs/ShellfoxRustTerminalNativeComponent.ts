import type { ViewProps, CodegenTypes } from 'react-native';
import { codegenNativeComponent } from 'react-native';

export interface NativeProps extends ViewProps {
  terminalHandle: string;
  readOnly?: boolean;
  focused?: boolean;
  selecting?: boolean;
  onInput?: CodegenTypes.DirectEventHandler<Readonly<{ terminalHandle: string }>>;
  onSelection?: CodegenTypes.DirectEventHandler<Readonly<{
    terminalHandle: string;
    text: string;
  }>>;
  onFileLink?: CodegenTypes.DirectEventHandler<Readonly<{
    terminalHandle: string;
    text: string;
    offset: CodegenTypes.Int32;
  }>>;
  modifiers?: CodegenTypes.Int32;
  fontSize?: CodegenTypes.Float;
  onState?: CodegenTypes.DirectEventHandler<Readonly<{
    terminalHandle: string;
    state: string;
    focusResult?: Readonly<{
      platform: string;
      source: string;
      attached: boolean;
      windowFocused: boolean;
      uiThread: boolean;
      inputAllowed: boolean;
      frameAvailable: boolean;
      focusRequest: string;
      focused: boolean;
      keyboardRequest: string;
    }>;
  }>>;
  onError?: CodegenTypes.DirectEventHandler<Readonly<{
    terminalHandle: string;
    code: CodegenTypes.Int32;
  }>>;
}

export default codegenNativeComponent<NativeProps>('ShellfoxRustTerminal');
