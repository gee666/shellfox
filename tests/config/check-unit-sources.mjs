// Static only: read/parse source, never import Vitest config, fixtures or application code.
import { readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';

const project = fileURLToPath(new URL('../..', import.meta.url));
const read = file => readFileSync(path.join(project, file), 'utf8');
const guardTest = 'tests/unit/app-process-boundary.test.ts';
const unitRoots = ['src', 'tests/unit', 'mobile/src'];
const unitInclude = ['src/**/*.test.{ts,tsx}', 'tests/unit/**/*.test.ts', 'mobile/src/**/*.test.{ts,tsx}'];
const heldSuffix = /\.(?:e2e|os)\.test\.tsx?$/;
const processObject = /^(?:process|global(?:This)?\.process)$/;
const processEffects = new Set(['kill', 'getBuiltinModule', 'binding', 'dlopen', 'exit', 'abort']);
const protectedBuiltin = /^(?:node:)?(?:child_process|module|process|worker_threads|cluster)$/;
const launchModule = /^(?:node-pty|electron|esbuild|playwright(?:-core)?|@playwright\/test|puppeteer(?:-core)?)$/;
const heldFixture = /(?:^tests\/e2e\/|\.(?:native|safety)-fixture(?:\.ts)?$|^tests\/fixtures\/(?:ssh-e2e-driver|cli-tty|electron)(?:\.ts)?$)/;
const fixtureModule = /(?:test-fixtures?|\.assertions)\.tsx?$|^tests\/(?:fixtures|unit)\//;

function parse(file, source = read(file)) {
  const ast = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true);
  if (ast.parseDiagnostics.length) throw new Error(`Cannot parse ${file}`);
  return ast;
}
function walk(node, visit) { visit(node); ts.forEachChild(node, child => walk(child, visit)); }
function literal(node) { return node && ts.isStringLiteralLike(node) ? node.text : undefined; }
function property(node) {
  if (ts.isPropertyAccessExpression(node)) return node.name.text;
  if (ts.isElementAccessExpression(node)) return literal(node.argumentExpression);
}
function reference(file, specifier) {
  if (specifier.startsWith('.')) return path.posix.normalize(path.posix.join(path.posix.dirname(file), specifier));
  return specifier;
}
function builtin(specifier, name) { return specifier === name || specifier === `node:${name}`; }
function shadowsProcess(node) {
  const namedProcess = binding => ts.isIdentifier(binding.name) && binding.name.text === 'process';
  for (let scope = node.parent; scope; scope = scope.parent) {
    if (ts.isFunctionLike(scope) && scope.parameters.some(namedProcess)) return true;
    if (ts.isForOfStatement(scope) && ts.isVariableDeclarationList(scope.initializer) && scope.initializer.declarations.some(namedProcess)) return true;
  }
  return false;
}

export function inspectUnitSource(file, source, heldPaths = []) {
  const ast = parse(file, source), problems = [], dependencies = new Set(), localActorMocks = new Set();
  const report = (node, reason) => problems.push(`${file}:${ast.getLineAndCharacterOfPosition(node.getStart(ast)).line + 1}: ${reason}`);
  const check = (node, specifier, { typeOnly = false, dynamic = false } = {}) => {
    if (typeOnly) return;
    if (!specifier) return report(node, 'dynamic module specifier needs review');
    const target = reference(file, specifier);
    if (heldPaths.some(held => held === target || held.replace(/\.tsx?$/, '') === target) ||
      heldSuffix.test(target + (target.endsWith('.test') ? '.ts' : '')) || heldFixture.test(target)) {
      report(node, `held test or actor fixture: ${specifier}`);
    }
    if (launchModule.test(specifier) && (dynamic || specifier !== 'electron' || !localActorMocks.has(specifier))) report(node, `direct actor/tool import: ${specifier}`);
    if (protectedBuiltin.test(specifier) && file !== guardTest &&
      (dynamic || !builtin(specifier, 'child_process') || !localActorMocks.has(specifier))) {
      report(node, `unreviewed process or loader import: ${specifier}`);
    }
    if (specifier.startsWith('.')) dependencies.add(target);
  };
  walk(ast, node => {
    if (!ts.isCallExpression(node) || node.expression.getText(ast) !== 'vi.mock') return;
    const specifier = literal(node.arguments[0]);
    if (!specifier || (!protectedBuiltin.test(specifier) && !launchModule.test(specifier))) return;
    const factory = node.arguments[1];
    if (!(builtin(specifier, 'child_process') || specifier === 'electron') || !factory ||
      !(ts.isArrowFunction(factory) || ts.isFunctionExpression(factory)) || factory.parameters.length) {
      report(node, 'process builtin mock must be a local fake without importOriginal');
      return;
    }
    localActorMocks.add(specifier);
  });
  walk(ast, node => {
    if (ts.isImportDeclaration(node) || (ts.isExportDeclaration(node) && node.moduleSpecifier)) {
      const clause = ts.isImportDeclaration(node) ? node.importClause : undefined;
      const typeOnly = ts.isExportDeclaration(node) ? node.isTypeOnly : clause?.isTypeOnly ||
        (!clause?.name && clause?.namedBindings && ts.isNamedImports(clause.namedBindings) &&
          clause.namedBindings.elements.length > 0 && clause.namedBindings.elements.every(item => item.isTypeOnly));
      check(node, literal(node.moduleSpecifier), { typeOnly });
    }
    if ((ts.isPropertyAccessExpression(node) || ts.isElementAccessExpression(node)) &&
      processObject.test(node.expression.getText(ast)) && processEffects.has(property(node)) &&
      !(node.expression.getText(ast) === 'process' && shadowsProcess(node))) {
      report(node, 'global process kill/probe or builtin-loader escape');
    }
    if (ts.isVariableDeclaration(node) && ts.isObjectBindingPattern(node.name) && node.initializer &&
      processObject.test(node.initializer.getText(ast)) && !shadowsProcess(node) &&
      node.name.elements.some(element => processEffects.has((element.propertyName ?? element.name).getText(ast)))) {
      report(node, 'destructured process effect');
    }
    if (ts.isNewExpression(node) && node.expression.getText(ast) === 'AutomationRuns') report(node, 'automation with the real default launcher');
    if (ts.isNewExpression(node) && node.expression.getText(ast) === 'PtyBackend' && !node.arguments?.length &&
      file !== 'src/main/terminal/backend-adoption.test.ts') report(node, 'default PTY needs review');
    if (!ts.isCallExpression(node)) return;
    const call = node.expression.getText(ast), args = node.arguments;
    if (node.expression.kind === ts.SyntaxKind.ImportKeyword) check(node, literal(args[0]), { dynamic: true });
    if (call === 'require' || call === 'module.require' || (call === 'createRequire' && file !== guardTest)) report(node, 'CJS escape');
    if (['vi.importActual', 'vi.unmock', 'vi.doUnmock'].includes(call)) {
      const specifier = literal(args[0]);
      if (!specifier || protectedBuiltin.test(specifier) || launchModule.test(specifier)) report(node, 'actual actor passthrough');
      else check(node, specifier);
    }
    if (call === 'vi.doMock' && protectedBuiltin.test(literal(args[0]) ?? '')) report(node, 'late process mock');
    if (call === 'startHandoff' && (file !== 'src/main/update/handoff-process.test.ts' || !localActorMocks.has('node:child_process'))) {
      report(node, 'handoff without the reviewed child model');
    }
    if (call === 'runAutomationCommand') report(node, 'automation CLI invocation needs a reviewed actor model');
    if (call === 'probeTerminalScope' && args.length < 2) report(node, 'system scope probe without injected command');
  });
  return { problems, dependencies: [...dependencies] };
}
function files(dir) {
  return readdirSync(path.join(project, dir), { withFileTypes: true }).flatMap(entry => {
    const file = `${dir}/${entry.name}`;
    return entry.isDirectory() ? files(file) : [file];
  });
}
function variable(ast, name) {
  let value;
  walk(ast, node => {
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && node.name.text === name) value = node.initializer;
  });
  return value;
}
function checkList(actual, expected, message) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) throw new Error(message);
}

export function selection() {
  const registry = parse('tests/config/integration-registry.ts');
  const rows = variable(registry, 'integrationFiles');
  if (!rows || !ts.isArrayLiteralExpression(rows)) throw new Error('Registry must be a literal array');
  const integration = rows.elements.map(node => {
    if (!ts.isCallExpression(node) || node.expression.getText(registry) !== 'held') throw new Error('Registry must contain held(path, kind, reason, platforms)');
    const [file, kind, reason, platforms] = node.arguments;
    if (!literal(file) || !literal(kind) || !literal(reason) || (platforms && !ts.isArrayLiteralExpression(platforms))) throw new Error('Incomplete integration classification');
    return { path: literal(file), kind: literal(kind), reason: literal(reason), platforms: platforms?.elements.map(literal) ?? ['win32', 'linux', 'darwin'] };
  });
  const heldPaths = integration.map(row => row.path), held = new Set(heldPaths);
  if (held.size !== integration.length) throw new Error('Duplicate integration path');
  const excludes = variable(registry, 'unitExcludes');
  if (!excludes || !ts.isArrayLiteralExpression(excludes)) throw new Error('No explicit unit excludes');
  checkList(excludes.elements.map(node => node.getText(registry)), ["'**/*.e2e.test.{ts,tsx}'", "'**/*.os.test.{ts,tsx}'", '...integrationPaths'], 'Unit excludes no longer use the registry');
  const candidates = unitRoots.flatMap(root => files(root).filter(file => (root === 'tests/unit' ? /\.test\.ts$/ : /\.test\.tsx?$/).test(file))).sort();
  for (const file of candidates) if (heldSuffix.test(file) && !held.has(file)) throw new Error(`Missing HOLD classification: ${file}`);
  for (const file of held) if (!candidates.includes(file)) throw new Error(`Integration path missing or outside original unit roots: ${file}`);
  const unit = candidates.filter(file => !held.has(file) && !heldSuffix.test(file));
  if (unit.some(file => held.has(file))) throw new Error('Unit and integration overlap');
  const config = parse('vitest.config.ts');
  let testOptions;
  walk(config, node => {
    if (ts.isPropertyAssignment(node) && node.name.getText(config) === 'test' && ts.isObjectLiteralExpression(node.initializer)) testOptions = node.initializer;
  });
  function option(name) {
    const value = testOptions?.properties.find(node => ts.isPropertyAssignment(node) && node.name.getText(config) === name)?.initializer;
    if (!value || !ts.isArrayLiteralExpression(value)) throw new Error(`Missing explicit Vitest ${name}`);
    return value.elements;
  }
  checkList(option('include').map(literal), unitInclude, 'Unit include changed; re-review source selection');
  checkList(option('exclude').map(node => node.getText(config)), ['...configDefaults.exclude', '...unitExcludes'], 'Unit config lost its registered exclusions');
  checkList(option('setupFiles').map(literal), ['tests/unit/setup-app-boundary.ts'], 'Unit config lost its boundary setup');
  return { unit, integration };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { unit, integration } = selection();
  const heldPaths = integration.map(row => row.path);
  const knownSources = new Set(unitRoots.flatMap(files).concat(files('tests/fixtures')));
  const queue = [...unit], scanned = new Set(), failures = [];
  for (const file of queue) {
    if (scanned.has(file)) continue;
    scanned.add(file);
    const result = inspectUnitSource(file, read(file), heldPaths);
    failures.push(...result.problems);
    for (const dependency of result.dependencies) {
      const source = [dependency, `${dependency}.ts`, `${dependency}.tsx`].find(name => knownSources.has(name) && fixtureModule.test(name));
      if (source) queue.push(source);
    }
  }
  if (failures.length) throw new Error(failures.join('\n'));
  console.log(`Static selection: ${unit.length} unit files, ${integration.length} integration HOLD files; ${scanned.size} unit test/fixture sources checked`);
}
