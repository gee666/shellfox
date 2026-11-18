-- Repository v1 layout from src/main/repository.ts. Later versions alter this copy.
CREATE TABLE sessions (
  id TEXT PRIMARY KEY, title TEXT NOT NULL, cwd TEXT NOT NULL,
  adapterId TEXT NOT NULL, shellId TEXT NOT NULL, shellExecutable TEXT NOT NULL,
  createdAt TEXT NOT NULL, updatedAt TEXT NOT NULL, settledAt TEXT,
  error TEXT, target TEXT
);
CREATE TABLE tabs (
  id TEXT PRIMARY KEY, sessionId TEXT NOT NULL REFERENCES sessions(id),
  title TEXT NOT NULL, cwd TEXT NOT NULL, ordinal INTEGER NOT NULL,
  createdAt TEXT NOT NULL, lifecycle TEXT NOT NULL, operationId TEXT NOT NULL,
  registration TEXT, error TEXT, UNIQUE(sessionId, ordinal)
);
CREATE TABLE operations (
  id TEXT PRIMARY KEY, sessionId TEXT NOT NULL REFERENCES sessions(id),
  tabId TEXT REFERENCES tabs(id), requestId TEXT UNIQUE, kind TEXT NOT NULL,
  state TEXT NOT NULL, createdAt TEXT NOT NULL, updatedAt TEXT NOT NULL, error TEXT
);
CREATE TABLE settings (id INTEGER PRIMARY KEY CHECK(id=1), value TEXT NOT NULL);
CREATE TABLE preferences (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE INDEX session_tabs ON tabs(sessionId, ordinal);
INSERT INTO sessions VALUES (
  'f919fb1a-fb03-4a93-8b9b-1cde465d5870', 'Legacy project', '/home/user/project',
  'embedded-pty', 'bash', '/bin/bash',
  '2025-01-01T01:02:03.000Z', '2025-02-02T02:03:04.000Z',
  '2025-02-03T03:04:05.000Z', NULL, '{"private":"source-only-secret"}'
);
INSERT INTO tabs VALUES (
  'f919fb1a-fb03-4a93-8b9b-1cde465d5871',
  'f919fb1a-fb03-4a93-8b9b-1cde465d5870', 'Saved live shell', '/home/user/project',
  0, '2025-01-01T01:02:03.000Z', 'open',
  'f919fb1a-fb03-4a93-8b9b-1cde465d5872', '{"pid":987654321}', NULL
);
INSERT INTO operations VALUES (
  'f919fb1a-fb03-4a93-8b9b-1cde465d5872',
  'f919fb1a-fb03-4a93-8b9b-1cde465d5870',
  'f919fb1a-fb03-4a93-8b9b-1cde465d5871', NULL, 'create', 'registered',
  '2025-01-01T01:02:03.000Z', '2025-01-01T01:02:03.000Z', NULL
);
INSERT INTO settings VALUES (1, '{
  "version":1, "autoUpdate":true, "accentColor":"#aBcD09",
  "backgroundColor":"#123456", "adapterId":"embedded-pty", "shellId":"bash",
  "shellExecutable":"/bin/bash", "processRules":[], "historyPageSize":10,
  "sessionSettleHours":24, "sessionDeleteHours":720,
  "terminalEnv":[{"name":"SECRET","value":"source-only-secret"}]
}');
INSERT INTO preferences VALUES ('window-state', '{"credential":"source-only-secret"}');
PRAGMA user_version=1;
