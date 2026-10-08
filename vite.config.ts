import { defineConfig } from 'vite';
import path from 'node:path';
import { readFileSync } from 'node:fs';
const { version } = JSON.parse(readFileSync(path.resolve('package.json'), 'utf8'));
export default defineConfig({plugins: [{ name: 'versioned-title', transformIndexHtml: html => html.replace('<title>Shellfox</title>', `<title>Shellfox v${version}</title>`) }], root: path.resolve('src/renderer'), base: './', cacheDir: path.resolve('tmp/build-cache/vite'), build: {outDir: path.resolve(process.env.SHELLFOX_BUILD_DIR || 'tmp/build', 'renderer'), emptyOutDir: true}, server: {host: '127.0.0.1'}});
