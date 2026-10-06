import { defineConfig } from '@playwright/test';
export default defineConfig({testDir:'.',testMatch:'window-lifecycle.e2e.ts',workers:1,retries:0,timeout:90000,outputDir:'../../tmp/window-lifecycle-playwright',reporter:'list',projects:[{name:'fake',grep:/fake (UI|confirmed)/},{name:'real',grep:/real native/}]});
