import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
const read = name => readFileSync(fileURLToPath(new URL(`../../${name}`, import.meta.url)), 'utf8');
const android = read('mobile/android/app/src/main/java/com/shellfox/mobile/ShellfoxDownloadsModule.kt');
const ios = read('mobile/ios/ShellfoxMobile/ShellfoxDownloads.mm');
test('Android publishes only exact-size pending Downloads and restart cleanup cannot delete completed files', () => {
  assert.match(android, /MediaStore\.Downloads\.EXTERNAL_CONTENT_URI/);
  assert.match(android, /IS_PENDING, 1/); assert.match(android, /IS_PENDING, 0/);
  assert.match(android, /current\.offset != current\.size/);
  assert.match(android, /resolver\.delete\(uri, "\$\{MediaStore\.MediaColumns\.IS_PENDING\}=1"/);
  assert.match(android, /journal\.getStringSet/); assert.match(android, /300_000/);
  assert.match(android, /Base64\.encodeToString\(bytes, Base64\.NO_WRAP\) != data/);
  assert.match(android, /Build\.VERSION\.SDK_INT < 29/);
  assert.match(read('mobile/android/app/src/main/java/com/shellfox/mobile/LegacyDownloadPicker.kt'), /ACTION_CREATE_DOCUMENT/);
});
test('iOS stages bounded chunks privately and uses the conventional Files export picker', () => {
  assert.match(ios, /initForExportingURLs:@\[current\.url\] asCopy:YES/);
  assert.match(ios, /current\.offset != current\.size/); assert.match(ios, /data\.length > SFDownloadChunk/);
  assert.match(ios, /removeItemAtURL:self\.cache/); assert.match(ios, /timeIntervalSinceReferenceDate - current\.touched > 300/);
  assert.match(ios, /documentPickerWasCancelled/); assert.match(ios, /completeExport:NO/);
});
test('native exports are registered on both platforms without a broad storage permission', () => {
  assert.match(read('mobile/android/app/src/main/java/com/shellfox/mobile/ShellfoxNetworkPackage.kt'), /ShellfoxDownloadsModule\(context\)/);
  assert.match(read('mobile/ios/ShellfoxMobile.xcodeproj/project.pbxproj'), /ShellfoxDownloads.mm in Sources/);
  assert.doesNotMatch(read('mobile/android/app/src/main/AndroidManifest.xml'), /WRITE_EXTERNAL_STORAGE|MANAGE_EXTERNAL_STORAGE/);
});
