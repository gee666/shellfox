// Runs in Electron, not Vitest's host Node. All database files stay under project tmp.
import { app } from 'electron';
import assert from 'node:assert/strict';
import path from 'node:path';
import { mkdirSync } from 'node:fs';
import { randomUUID } from 'node:crypto';
import { Repository, SCHEMA_VERSION } from './repository';
import { EmbeddedSessionService } from './terminal/service';
import { defaultSettings } from './defaults';
import type { SessionRecord, TabRecord } from './models';
declare const __PROJECT_ROOT__: string;
const folder = path.join(__PROJECT_ROOT__, 'tmp', 'storage-tests', randomUUID());
mkdirSync(folder, { recursive: true });
app.setPath('userData', folder);
async function run() {
  await app.whenReady();
  const filename = path.join(folder, 'manager.sqlite3');
  let repository = new Repository(filename);
  assert.equal(repository.db.pragma('user_version', {simple:true}),SCHEMA_VERSION);
  assert.equal(repository.db.pragma('journal_mode', {simple:true}),'wal');
  assert.equal(repository.db.pragma('foreign_keys', {simple:true}),1);
  assert.deepEqual(repository.settings(),defaultSettings);
  const now = new Date().toISOString();
  const s: SessionRecord = {id:randomUUID(),title:"100%_ 雪 ' OR 1=1 --",cwd:'C:\\same',adapterId:'windows-terminal',shellId:'pwsh',shellExecutable:'C:\\PowerShell\\pwsh.exe',createdAt:now,updatedAt:now,settledAt:now,error:null,target:null};
  const tab: TabRecord = {id:randomUUID(),sessionId:s.id,title:'Shell 1',cwd:s.cwd,ordinal:0,createdAt:now,lifecycle:'launching',operationId:randomUUID(),registration:null,error:null};
  const requestId=randomUUID();
  repository.transaction(()=>{repository.saveSession(s);repository.saveTab(tab);repository.saveOperation({id:tab.operationId,sessionId:s.id,tabId:tab.id,requestId,kind:'create',state:'intent',createdAt:now,updatedAt:now,error:null});});
  assert.equal(repository.sessionByRequest(requestId)?.id,s.id);
  assert.equal(repository.tabs(s.id).length,1);
  assert.equal(repository.settled('%_')[0].id,s.id);
  assert.equal(repository.settled("' OR 1=1 --")[0].id,s.id);
  assert.equal(repository.settled('no match').length,0);
  assert.throws(()=>repository.saveTab({...tab,id:randomUUID(),sessionId:randomUUID()}));
  assert.throws(()=>repository.transaction(()=>{repository.saveSession({...s,id:'rollback',title:'rollback'});throw new Error('rollback');}));
  assert.equal(repository.session('rollback'),undefined);
  repository.saveSession({ ...s, env: [{ name: 'SHELLFOX_STORAGE_TEST', value: 'persisted' }] });
  repository.saveSettings({...defaultSettings,accentColor:'#123456'});
  repository.saveExplorerPreference(true);
  assert.equal(repository.windowState(),undefined);
  repository.saveWindowState({x:1,y:2,width:900,height:700,maximized:true});
  repository.close();
  repository=new Repository(filename);
  assert.equal(repository.settings().accentColor,'#123456');
  assert.deepEqual(repository.windowState(),{x:1,y:2,width:900,height:700,maximized:true});
  assert.equal(repository.explorerPreference(),true);
  assert.equal(repository.session(s.id)?.title,s.title);
  assert.equal(repository.operation(tab.operationId)?.state,'intent');
  assert.deepEqual(repository.session(s.id)?.env, [{ name: 'SHELLFOX_STORAGE_TEST', value: 'persisted' }]);
  repository.saveTab({...tab,lifecycle:'closed',terminal:{kind:'embedded',profileId:'login-shell',exitCode:7}});
  repository.close();repository=new Repository(filename);
  assert.deepEqual(repository.tab(tab.id)?.terminal,{kind:'embedded',profileId:'login-shell',exitCode:7});
  // Real SQLite: metadata writes, ordinal swaps, rollback and restart persistence.
  {
    const saved: SessionRecord = { ...s, id: randomUUID(), adapterId: 'embedded-pty', settledAt: null };
    const tabs: TabRecord[] = [0, 1, 2].map(ordinal => ({ ...tab, id: randomUUID(), sessionId: saved.id, ordinal, lifecycle: 'closed', terminal: { kind: 'embedded', profileId: 'login-shell', exitCode: ordinal } }));
    repository.transaction(() => { repository.saveSession(saved); tabs.forEach(t => repository.saveTab(t)); });
    let service = new EmbeddedSessionService(repository, undefined, () => ({ setWatch: () => {}, dispose: () => {} }));
    assert.equal((await service.renameTab({ sessionId: saved.id, tabId: tabs[0].id, title: 'Build 雪' })).ok, true);
    const order = [tabs[2].id, tabs[0].id, tabs[1].id];
    assert.equal((await service.reorderTabs({ sessionId: saved.id, tabIds: order })).ok, true);
    assert.deepEqual(repository.tabs(saved.id).map(t => [t.id, t.ordinal]), order.map((id, ordinal) => [id, ordinal]));
    const before = repository.tabs(saved.id), beforeSession = repository.session(saved.id);
    const saveSession = repository.saveSession.bind(repository);
    repository.saveSession = () => { throw new Error('disk full'); };
    assert.equal((await service.renameTab({ sessionId: saved.id, tabId: tabs[0].id, title: 'unsaved' })).ok, false);
    assert.equal((await service.reorderTabs({ sessionId: saved.id, tabIds: [...order].reverse() })).ok, false);
    assert.deepEqual(repository.tabs(saved.id), before);
    assert.deepEqual(repository.session(saved.id), beforeSession);
    repository.saveSession = saveSession;
    await service.dispose(); repository.close(); repository = new Repository(filename);
    assert.deepEqual(repository.tabs(saved.id), before);
    assert.equal(repository.tab(tabs[0].id)?.title, 'Build 雪');
    assert.equal(repository.tab(tabs[0].id)?.terminal?.userTitle, 'Build 雪');
    service = new EmbeddedSessionService(repository, undefined, () => ({ setWatch: () => {}, dispose: () => {} }));
    assert.equal((await service.renameTab({ sessionId: saved.id, tabId: tabs[1].id, title: 'After restart' })).ok, true);
    assert.equal(repository.tab(tabs[1].id)?.title, 'After restart');
    await service.dispose();
    repository.saveSession({ ...saved, settledAt: now }); assert.equal(repository.deleteSession(saved.id), true);
  }
  // Permanent deletion removes an archived session with every dependent row and refuses live sessions.
  {
    const doomed:SessionRecord={...s,id:randomUUID(),title:'doomed',settledAt:now,binding:null,windowState:'unknown'};
    const live:SessionRecord={...s,id:randomUUID(),title:'live',settledAt:null};
    const doomedTab:TabRecord={...tab,id:randomUUID(),sessionId:doomed.id,operationId:randomUUID(),member:null,terminal:{kind:'embedded',profileId:'login-shell',exitCode:0}};
    const liveTab:TabRecord={...tab,id:randomUUID(),sessionId:live.id,operationId:randomUUID()};
    const count=(sql:string,...args:unknown[])=>(repository.db.prepare(sql).get(...args) as {n:number}).n;
    repository.transaction(()=>{
      repository.saveSession(doomed);repository.saveSession(live);repository.saveTab(doomedTab);repository.saveTab(liveTab);
      repository.saveOperation({id:doomedTab.operationId,sessionId:doomed.id,tabId:doomedTab.id,requestId:randomUUID(),kind:'create',state:'registered',createdAt:now,updatedAt:now,error:null});
      repository.saveOperation({id:liveTab.operationId,sessionId:live.id,tabId:liveTab.id,requestId:randomUUID(),kind:'create',state:'registered',createdAt:now,updatedAt:now,error:null});
    });
    assert.equal(count("SELECT count(*) n FROM preferences WHERE key IN (?,?)",'session-window:'+doomed.id,'tab-member:'+doomedTab.id),2);
    assert.equal(count('SELECT count(*) n FROM terminal_metadata WHERE tabId=?',doomedTab.id),1);
    assert.equal(repository.deleteSession(live.id),false);
    assert.equal(repository.deleteSession(randomUUID()),false);
    assert.ok(repository.session(live.id));assert.equal(repository.tabs(live.id).length,1);
    assert.equal(repository.deleteSession(doomed.id),true);
    assert.equal(repository.session(doomed.id),undefined);assert.equal(repository.tabs(doomed.id).length,0);
    assert.equal(repository.operation(doomedTab.operationId),undefined);
    assert.equal(count('SELECT count(*) n FROM terminal_metadata WHERE tabId=?',doomedTab.id),0);
    assert.equal(count("SELECT count(*) n FROM preferences WHERE key IN (?,?)",'session-window:'+doomed.id,'tab-member:'+doomedTab.id),0);
    assert.ok(repository.operation(liveTab.operationId));assert.equal(repository.tabs(live.id).length,1);
    assert.equal(repository.deleteSession(doomed.id),false);
    repository.db.exec("DELETE FROM operations WHERE sessionId='"+live.id+"'; DELETE FROM tabs WHERE sessionId='"+live.id+"'; DELETE FROM sessions WHERE id='"+live.id+"'");
  }
  // Simulate the previous version's schema and verify a real migration keeps task/root history.
  // Pinned state survives a close/reopen at the current schema version (reopening must not be refused).
  repository.saveSession({...s,pinnedAt:now});repository.close();repository=new Repository(filename);
  assert.equal(repository.session(s.id)?.pinnedAt,now);
  repository.saveSession({...repository.session(s.id)!,pinnedAt:null});assert.equal(repository.session(s.id)?.pinnedAt,null);
  repository.db.exec('DROP TABLE terminal_metadata; ALTER TABLE sessions DROP COLUMN env; ALTER TABLE sessions DROP COLUMN pinnedAt');repository.db.pragma('user_version = 1');repository.close();
  repository=new Repository(filename);
  assert.equal(repository.db.pragma('user_version',{simple:true}),SCHEMA_VERSION);
  assert.equal(repository.session(s.id)?.pinnedAt,null);
  repository.close();repository=new Repository(filename);
  assert.equal(repository.db.pragma('user_version',{simple:true}),SCHEMA_VERSION);
  assert.equal(repository.session(s.id)?.title,s.title);
  assert.equal(repository.tab(tab.id)?.terminal,null);
  const before=repository.sessions().length;
  for(let i=0;i<24;i++)repository.saveSession({...s,id:randomUUID(),title:'page '+i});
  assert.equal(repository.settled('').length,before+24);
  const ids=repository.settled('').map(s=>s.id);
  assert.deepEqual(ids,[...ids].sort());
  repository.close();
  console.log('SQLite Electron smoke: storage and embedded metadata/migration assertions passed; DB '+filename);
  app.exit(0);
}
void run().catch(()=>{console.error('SQLite Electron smoke failed');app.exit(1);});
