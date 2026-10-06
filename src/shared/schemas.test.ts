import { it, expect } from 'vitest';
import { historyQuerySchema, nativeEventSchema, pathSchema, requestEnvelopeSchema, requestSchemas, settingsSchema } from './schemas';
import { defaultSettings } from '../main/defaults';
it('rejects unexpected IPC fields and arbitrary methods', () => {
  expect(requestEnvelopeSchema.safeParse({version:1,method:'execute',payload:{}}).success).toBe(false);
  expect(requestSchemas.createSession.safeParse({cwd:'C:\\work',requestId:'c36951c8-03f5-49c7-bb11-ad8828044a31',command:'evil'}).success).toBe(false);
});
it('rejects malformed IDs', () => expect(requestSchemas.focusSession.safeParse({sessionId:'1'}).success).toBe(false));
it.each(['relative','\\\\server\\folder','\\\\?\\C:\\folder','C:\\x\u0000','shell:desktop'])('rejects unsafe path %s', value => expect(pathSchema.safeParse(value).success).toBe(false));
it('treats shell punctuation as path data', () => expect(pathSchema.safeParse("C:\\space 雪\\a'&;%[x]").success).toBe(true));
it('bounds pagination and literal search strings', () => {
  expect(historyQuerySchema.safeParse({search:'%_',status:'all',page:1,pageSize:100}).success).toBe(true);
  expect(historyQuerySchema.safeParse({search:'',status:'all',page:0,pageSize:101}).success).toBe(false);
});
it('validates accents and structured process rules', () => {
  expect(settingsSchema.safeParse(defaultSettings).success).toBe(true);
  expect(settingsSchema.safeParse({...defaultSettings,accentColor:'pink'}).success).toBe(false);
  expect(settingsSchema.safeParse({...defaultSettings,processRules:[{...defaultSettings.processRules[0],executableBasenames:[],executablePaths:[]}]}).success).toBe(false);
  expect(settingsSchema.safeParse({...defaultSettings,processRules:[{...defaultSettings.processRules[0],regex:'.*'}]}).success).toBe(false);
});
it('loads settings saved before backgroundColor existed with the default background', () => {
  const { backgroundColor, ...legacy } = defaultSettings;
  const parsed = settingsSchema.safeParse(legacy);
  expect(backgroundColor).toBe('#111016');
  expect(parsed.success && parsed.data.backgroundColor).toBe('#111016');
  // Same path as Repository.settings(): JSON.parse of the stored row, then settingsSchema.parse.
  expect(settingsSchema.parse(JSON.parse(JSON.stringify(legacy))).backgroundColor).toBe('#111016');
  expect(settingsSchema.parse({ ...legacy, backgroundColor: '#ffffff' }).backgroundColor).toBe('#ffffff');
  expect(settingsSchema.safeParse({ ...defaultSettings, backgroundColor: 'white' }).success).toBe(false);
  expect(settingsSchema.safeParse({ ...defaultSettings, backgroundColor: '#fff' }).success).toBe(false);
});
it.each(['..\\agent\\cli.js','agent/../cli.js','agent/./cli.js','.','..','   '])('rejects native-invalid script suffix %s', suffix => {
  expect(settingsSchema.safeParse({...defaultSettings,processRules:[{...defaultSettings.processRules[0],scriptPathSuffixes:[suffix]}]}).success).toBe(false);
});
it('accepts path-component suffixes with either separator and dot-prefixed package names', () => {
  expect(settingsSchema.safeParse({...defaultSettings,processRules:[{...defaultSettings.processRules[0],scriptPathSuffixes:['agent/cli.js','agent\\cli.js','.agent/cli.js']}]}).success).toBe(true);
});
it('rejects whitespace-only executable basenames', () => {
  expect(settingsSchema.safeParse({...defaultSettings,processRules:[{...defaultSettings.processRules[0],executableBasenames:['  ']}]}).success).toBe(false);
});
it('rejects oversized observations and unknown fields', () => {
  expect(nativeEventSchema.safeParse({type:'observations',items:new Array(1001).fill({})}).success).toBe(false);
  expect(nativeEventSchema.safeParse({type:'unavailable',error:{code:'NATIVE_UNAVAILABLE',message:'Failed',retryable:true},token:'secret'}).success).toBe(false);
});
