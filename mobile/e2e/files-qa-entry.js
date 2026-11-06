import 'react-native-get-random-values';
import 'fast-text-encoding';
import 'react-native-url-polyfill/auto';
import { AppRegistry } from 'react-native';
import FilesTestLauncher from './FilesTestLauncher';
import { name as appName } from '../app.json';
AppRegistry.registerComponent(appName, () => FilesTestLauncher);
