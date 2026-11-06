// Final user-facing fixture install after native QA/visual signoff. Never touches the normal client.
import { androidFilesDriver } from './android-files-driver.mjs';
const d = androidFilesDriver('mobile-files-demo');
d.requireApks();
await d.restoreDemo();
console.log('PASS Demo opens the actual browser, file Close returns to its origin directory, root Back shows the compact workspace, and no QA tools appear.');
console.log(`Artifacts: ${d.output}`);
