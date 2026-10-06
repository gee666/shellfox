import { defineConfig } from 'vite';
import path from 'node:path';
export default defineConfig({root: path.resolve('src/renderer'), base: './', cacheDir: path.resolve('tmp/build-cache/vite'), build: {outDir: path.resolve(process.env.SHELLFOX_BUILD_DIR || 'tmp/build', 'renderer'), emptyOutDir: true}, server: {host: '127.0.0.1'}});
