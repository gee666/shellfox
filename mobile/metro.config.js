const path = require('node:path');
const { getDefaultConfig, mergeConfig } = require('@react-native/metro-config');

const root = path.resolve(__dirname, '..');
const scratch = path.join(root, 'tmp').replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
module.exports = mergeConfig(getDefaultConfig(__dirname), {
  watchFolders: [root],
  resolver: {
    nodeModulesPaths: [path.join(root, 'node_modules')],
    blockList: new RegExp(`^${scratch}(?:[/\\\\]|$)`),
  },
});
