const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const {webcrypto} = require('node:crypto');
const html = fs.readFileSync(require.resolve('../assets/diagnostics.html'), 'utf8');
const script = /<script>([\s\S]*?)<\/script>/.exec(html)[1];

function loadPage(storage) {
  const nodes = {};
  const events = {};
  const reports = [];
  const context = {
    // A non-HTTPS hostname has getRandomValues but not randomUUID in WebKit.
    crypto: {getRandomValues: bytes => webcrypto.getRandomValues(bytes)},
    localStorage: {getItem: key => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value)},
    document: {readyState: 'complete', getElementById: id => nodes[id] ??= {}},
    location: {pathname: '/fixture'},
    window: {ipc: {postMessage: value => reports.push(JSON.parse(value))}, addEventListener: (name, handler) => events[name] = handler},
  };
  vm.runInNewContext(script, context);
  events.pageshow();
  return reports[0].marker;
}

test('diagnostic pages report readiness without secure-context randomUUID', () => {
  assert.match(loadPage(new Map()), /^[0-9a-f-]{32,36}$/);
});

test('diagnostic markers persist within an account and differ across stores', () => {
  const account = new Map();
  const marker = loadPage(account);
  assert.equal(loadPage(account), marker);
  assert.notEqual(loadPage(new Map()), marker);
});
