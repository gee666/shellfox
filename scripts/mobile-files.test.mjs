import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
const read = path => readFileSync(new URL('../' + path, import.meta.url), 'utf8');
const android = read('mobile/android/app/src/main/java/com/shellfox/mobile/ShellfoxFilesModule.kt');
const ios = read('mobile/ios/ShellfoxMobile/ShellfoxFiles.mm');

test('Android picker uses scoped document permissions, not whole-storage grants', () => {
  assert.match(android, /Intent\.ACTION_OPEN_DOCUMENT/);
  assert.doesNotMatch(android, /addCategory\(Intent\.CATEGORY_OPENABLE\)/);
  assert.match(android, /type = "\*\/\*"/);
  assert.match(android, /takePersistableUriPermission/);
  assert.match(android, /releasePersistableUriPermission/);
  assert.match(android, /getSharedPreferences\("terminal-attachment-grants"/);
  assert.match(android, /preferences\.getStringSet\("uris", emptySet\(\)\)\?\.toList\(\)\?\.forEach/);
  assert.match(android, /owned\.contains\(uri\.toString\(\)\)/);
  assert.match(android, /persistedUriPermissions\.any \{ it\.uri == uri && it\.isReadPermission \}/);
  assert.doesNotMatch(android, /persistedUriPermissions\.forEach/);
  assert.doesNotMatch(read('mobile/android/app/src/main/AndroidManifest.xml'), /MANAGE_EXTERNAL_STORAGE|READ_EXTERNAL_STORAGE|READ_MEDIA_IMAGES/);
});
test('Android provider downloads and reads are bounded on a worker and only opaque handles reach JS', () => {
  assert.match(android, /newSingleThreadExecutor/);
  assert.match(android, /ByteArray\(64 \* 1024\)/);
  assert.match(android, /size > MAX_FILE/);
  assert.match(android, /length > MAX_CHUNK/);
  assert.match(android, /handles\[handle\]/);
  assert.match(android, /input\.seek\(offset\.toLong\(\)\); input\.readFully\(bytes\)/);
  assert.match(android, /Base64\.NO_WRAP/);
  assert.match(android, /current\.cancelled\.get\(\)/);
  assert.match(android, /onHostDestroy\(\) \{ cleanup\(false\) \}/);
  assert.match(android, /worker\.shutdown\(\)/);
  assert.match(android, /requestCode != current\.requestCode/);
});
test('Android cancellation interrupts provider opens and closes active streams away from UI and copy workers', () => {
  assert.match(android, /val signal = CancellationSignal\(\)/);
  assert.match(android, /resolver\.query\(uri, columns, null, null, null, current\.signal\)/);
  assert.match(android, /openTypedAssetFileDescriptor\(uri, mime, null, current\.signal\)/);
  assert.match(android, /openAssetFileDescriptor\(uri, "r", current\.signal\)/);
  assert.doesNotMatch(android, /openInputStream\(/);
  assert.match(android, /private val providers = ThreadPoolExecutor\(2, 2,[\s\S]*SynchronousQueue<Runnable>/);
  assert.match(android, /private val cancellations = ThreadPoolExecutor\(4, 4,[\s\S]*ArrayBlockingQueue<Runnable>\(8\)/);
  assert.match(android, /cancelTask \{ current\.signal\.cancel\(\) \}/);
  assert.match(android, /current\.resource\.getAndSet\(null\)\?\.let \{ cancelTask \{ it\.close\(\) \} \}/);
  assert.match(android, /submit \{ current\.file\?\.delete\(\); releaseGrant\(current\) \}/);
  assert.match(android, /\(copies \+ listOfNotNull\(selection\)\)\.forEach \{ cancel\(it\) \}/);
});
test('iOS supports Files and permissionless Photos, including cloud-provider representations', () => {
  assert.match(ios, /initForOpeningContentTypes:@\[UTTypeItem\] asCopy:NO/);
  assert.match(ios, /PHPickerViewController/);
  assert.match(ios, /\[PHPickerFilter imagesFilter\]/);
  assert.match(ios, /loadFileRepresentationForTypeIdentifier:UTTypeImage.identifier/);
  assert.match(ios, /dispatch_sync\(self.worker/);
  assert.match(ios, /coordinateReadingItemAtURL/);
  assert.match(ios, /startAccessingSecurityScopedResource/);
  assert.match(ios, /@finally \{[\s\S]*if \(scoped\) \[url stopAccessingSecurityScopedResource\]/);
});
test('iOS native reads and caches are bounded, cancelled handles cannot outlive invalidation', () => {
  assert.match(ios, /size > SFMaxFile/);
  assert.match(ios, /length > SFMaxChunk/);
  assert.match(ios, /self\.closed \? nil : self\.handles\[handle\]/);
  assert.match(ios, /readDataUpToLength:\(NSUInteger\)length/);
  assert.match(ios, /base64EncodedStringWithOptions:0/);
  assert.match(ios, /removeItemAtURL:destination/);
  assert.match(ios, /- \(void\)invalidate/);
  assert.match(ios, /\[self.handles removeAllObjects\]/);
  assert.match(ios, /controller != self\.picker/);
});
test('native file modules are compiled into both applications', () => {
  assert.match(read('mobile/android/app/src/main/java/com/shellfox/mobile/ShellfoxNetworkPackage.kt'), /ShellfoxFilesModule\(context\)/);
  const project = read('mobile/ios/ShellfoxMobile.xcodeproj/project.pbxproj');
  assert.match(project, /ShellfoxFiles.mm in Sources/);
  assert.match(project, /path = ShellfoxMobile\/ShellfoxFiles.mm/);
});
