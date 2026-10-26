// Build only with ENTRY_FILE=e2e/terminal-entry.js. Never use for release builds.
import 'react-native-get-random-values';
import 'fast-text-encoding';
import 'react-native-url-polyfill/auto';
import { AppRegistry } from 'react-native';
import TerminalHarness from './TerminalHarness';
AppRegistry.registerComponent('ShellfoxMobile', () => TerminalHarness);
