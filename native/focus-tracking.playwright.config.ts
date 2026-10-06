import { defineConfig } from '@playwright/test';
import path from 'node:path';
export default defineConfig({testDir:'./focus-tracking-tests',outputDir:path.resolve('tmp/focus-tracking/artifacts'),workers:1,retries:0,timeout:120000,reporter:[['list'],['json',{outputFile:path.resolve('tmp/focus-tracking/report.json')}]],projects:[{name:'windows'}]});
