// Builds only the isolated terminalTest APK. Normal mobile client data/signing is untouched.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { closeSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, openSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../', import.meta.url));
const mode = process.argv[2] ?? 'both';
assert(['qa', 'demo', 'remote', 'both'].includes(mode), 'Usage: node mobile/e2e/build-files-android.mjs [qa|demo|remote|both]');
assert(process.env.JAVA_HOME && process.env.ANDROID_HOME, 'JAVA_HOME and ANDROID_HOME are required.');
const output = path.join(root, 'tmp/mobile-e2e'); mkdirSync(output, { recursive: true });
const java = path.join(process.env.JAVA_HOME, 'bin', process.platform === 'win32' ? 'java.exe' : 'java');
const aapt = path.join(process.env.ANDROID_HOME, 'build-tools/36.0.0', process.platform === 'win32' ? 'aapt.exe' : 'aapt');
const apk = path.join(root, 'mobile/android/app/build/outputs/apk/terminalTest/app-terminalTest.apk');
for (const entry of mode === 'both' ? ['qa', 'demo'] : [mode]) {
  const entryFile = entry === 'qa' ? 'e2e/files-qa-entry.js' : entry === 'remote' ? 'e2e/files-remote-entry.js' : 'e2e/files-entry.js';
  const init = entry === 'remote' ? path.join(output, 'files-real-remote.init.gradle') : undefined;
  if (init) {
    const overlay = path.join(root, 'mobile/e2e/remote-android');
    writeFileSync(init, `gradle.beforeProject { p ->\n  if (p.path == ':app') {\n    if (System.getenv('ENTRY_FILE') != 'e2e/files-remote-entry.js') throw new GradleException('QA overlay requires the explicit real-remote entry.')\n    if (gradle.startParameter.taskNames != [':app:assembleTerminalTest']) throw new GradleException('QA overlay only assembles terminalTest.')\n    p.plugins.withId('com.android.application') {\n      def qa = p.android.sourceSets.maybeCreate('terminalTest')\n      qa.manifest.srcFile(${JSON.stringify(path.join(overlay, 'AndroidManifest.xml'))})\n      qa.res.srcDir(${JSON.stringify(path.join(overlay, 'res'))})\n    }\n  }\n}\n`);
  }
  const log = path.join(output, `build-files-${entry}.log`), descriptor = openSync(log, 'w');
  console.log(`Building ${entryFile}; log ${path.relative(root, log)}`);
  try {
    execFileSync(java, [`-Djava.io.tmpdir=${path.join(root, 'tmp')}`, '-classpath', 'gradle/wrapper/gradle-wrapper.jar',
      'org.gradle.wrapper.GradleWrapperMain', '--no-daemon', '--console=plain', ...(init ? ['--init-script', init] : []),
      `-PreactNativeArchitectures=${process.env.SHELLFOX_ANDROID_ARCHS ?? 'x86_64'}`, ':app:assembleTerminalTest'], {
      cwd: path.join(root, 'mobile/android'), timeout: 600_000, windowsHide: true, stdio: ['ignore', descriptor, descriptor],
      env: { ...process.env, ENTRY_FILE: entryFile, TMP: path.join(root, 'tmp'), TEMP: path.join(root, 'tmp'), TMPDIR: path.join(root, 'tmp'), GRADLE_USER_HOME: path.join(root, 'tmp/gradle-home') },
    });
  } finally { closeSync(descriptor); if (init) rmSync(init, { force: true }); }
  assert(existsSync(apk), 'Gradle did not produce the isolated fixture APK.');
  const manifest = execFileSync(aapt, ['dump', 'badging', apk], { encoding: 'utf8', timeout: 30_000 });
  assert.match(manifest, /^package: name='com\.shellfox\.mobile\.terminaltest' /m, 'Refusing to archive a normal mobile APK.');
  const inspection = mkdtempSync(path.join(output, 'inspect-entry-'));
  try {
    const jar = path.join(process.env.JAVA_HOME, 'bin', process.platform === 'win32' ? 'jar.exe' : 'jar');
    // AAPT shortens ZIP paths; the resource table retains the authoritative XML name.
    const resources = execFileSync(aapt, ['dump', 'resources', apk], { encoding: 'utf8', timeout: 30_000, windowsHide: true, maxBuffer: 16 * 1024 * 1024 });
    assert.equal(resources.includes(':xml/files_remote_qa_network_security:'), entry === 'remote', 'Loopback cleartext XML must exist only in the explicit real-remote QA APK.');
    execFileSync(jar, ['xf', apk, 'assets/index.android.bundle'], { cwd: inspection, timeout: 30_000, windowsHide: true });
    const bundle = readFileSync(path.join(inspection, 'assets/index.android.bundle'));
    const qaLabels = ['Files QA launcher', 'Open files fixture', 'Open host files fixture', 'Stage pending download fixture', 'Read clipboard fixture'];
    if (entry === 'qa') assert(bundle.includes(Buffer.from(qaLabels[0])), 'QA APK did not bundle the explicit QA entry.');
    else if (entry === 'remote') assert(bundle.includes(Buffer.from('http://10.0.2.2:41973')), 'Real-remote APK must contain only its fixed QA transport origin.');
    else for (const label of qaLabels) assert(!bundle.includes(Buffer.from(label)), `Demo APK must not contain QA launcher controls: ${label}`);
  } finally { rmSync(inspection, { recursive: true, force: true }); }
  const archived = path.join(output, `files-${entry}.apk`); copyFileSync(apk, archived);
  console.log(`Archived ${path.relative(root, archived)}`);
}
