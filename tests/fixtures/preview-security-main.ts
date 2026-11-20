import { app, BrowserWindow } from 'electron';
import { mkdirSync } from 'node:fs';
import path from 'node:path';
import { installDesktopPolicy, secureDesktopWindow } from '../../src/main/desktop-security';
import { registerFilePreviewScheme } from '../../src/main/file-preview-protocol';

const profile = path.join(__dirname, 'profile');
mkdirSync(profile, { mode: 0o700 });
app.setPath('userData', profile);
registerFilePreviewScheme();
void app.whenReady().then(async () => {
  installDesktopPolicy();
  const window = new BrowserWindow({ show: false, webPreferences: { sandbox: true, contextIsolation: true } });
  secureDesktopWindow(window);
  await window.loadFile(path.join(__dirname, 'preview.html'));
});
