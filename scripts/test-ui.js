// UI behavior checks without starting Tauri or changing the user's addons.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const nodes = new Map();
function element(id) {
  if (!nodes.has(id)) nodes.set(id, {
    value: '', checked: false, textContent: '', innerHTML: '', dataset: {},
    classList: { add() {}, remove() {}, toggle() {} },
    listeners: {}, attributes: {}, addEventListener(type, fn) { this.listeners[type] = fn; },
    setAttribute(name, value) { this.attributes[name] = String(value); },
    removeAttribute(name) { delete this.attributes[name]; },
    focus() { document.activeElement = this; },
  });
  return nodes.get(id);
}
const makeSortHeads = (prefix, fields) => fields.map((by) => {
  const head = element(`${prefix}-${by}`);
  head.dataset.sort = by;
  head.textContent = by === 'name' ? 'Addon' : by[0].toUpperCase() + by.slice(1);
  return head;
});
const sortHeads = makeSortHeads('sort', ['name', 'category', 'downloads', 'source']);
const installedSortHeads = makeSortHeads('installed-sort', ['name', 'source', 'status']);
element('browse-sort').value = 'name';
const document = {
  getElementById: element,
  querySelectorAll: (selector) => ({ '.sort-head': sortHeads, '.installed-sort-head': installedSortHeads })[selector] || [],
  querySelector: () => element('main'),
  addEventListener() {}, removeEventListener() {}, activeElement: element('previous'),
};
let detailReply;
let invokeReply = () => Promise.resolve();
const invokeCalls = [];
// The file dialog and Tauri's drag-drop events, as the app sees them.
let dialogReply = () => Promise.resolve(null);
const dialogCalls = [];
const tauriListeners = {};
const context = vm.createContext({
  document, window: { __TAURI__: { core: { invoke(command, args) {
    invokeCalls.push({ command, args });
    if (command === "addon_details") return detailReply;
    return invokeReply(command, args);
  } }, opener: {},
  dialog: { open(options) { dialogCalls.push(options); return dialogReply(options); } },
  event: { listen(name, handler) { tauriListeners[name] = handler; return Promise.resolve(() => {}); } } } },
  setTimeout, clearTimeout, console,
});
const source = fs.readFileSync('ui/app.js', 'utf8').split('// ---------------------------------------------------------------- boot')[0];
vm.runInContext(source, context);
const run = (code) => vm.runInContext(code, context);
run(`packages = [
  {key:'a',name:'Alpha <addon>',folders:['Alpha'],author:'Ada',source_label:'GitHub',source:true,status:'update',supports_forever:true,missing_deps:[],installed_version:'1',remote_version:'2'},
  {key:'b',name:'Beta',folders:['Beta'],source_label:'Wago',status:'no-key',supports_forever:true,missing_deps:[]},
  {key:'c',name:'Gamma',folders:['Gamma'],source_label:'',status:'ok',supports_forever:true,missing_deps:[],ignored:true},
  {key:'d',name:'Delta',folders:['Delta'],source_label:'CurseForge',source:true,status:'update',supports_forever:true,missing_deps:[],pinned:true}
]; renderInstalled();`);
assert.equal(String(element('stat-total').textContent), '4');
assert.equal(String(element('stat-updates').textContent), '2');
assert.equal(String(element('stat-attention').textContent), '1');
assert.equal(element('btn-update-all').textContent, 'Update all (1)');
assert.match(element('installed-list').innerHTML, /Alpha &lt;addon&gt;/);
assert.doesNotMatch(element('installed-list').innerHTML, /c-ver|Installed version|Available version/);
run(`packages[0].notice = 'Beta workaround <b>'; renderInstalled();`);
assert.match(element('installed-list').innerHTML, /note warn">Beta workaround &lt;b&gt;/);
run(`delete packages[0].notice; renderInstalled();`);
assert.doesNotMatch(element('installed-list').innerHTML, /Gamma/);
assert.equal(run(`activeFilter = 'updates'; visiblePackages().length`), 2);
assert.equal(run(`activeFilter = 'attention'; visiblePackages()[0].key`), 'b');
element('installed-filter').value = 'Ada';
assert.equal(run(`activeFilter = 'all'; visiblePackages().length`), 1);
element('installed-filter').value = 'unmatched';
run('renderInstalled()');
assert.match(element('installed-list').innerHTML, /Show all addons/);
element('installed-filter').value = '';
element('show-ignored').checked = true;
assert.equal(run('visiblePackages().length'), 4);
const installedKeys = () => run('visiblePackages().map(p => p.key).join()');
assert.equal(element('installed-sort-status').attributes['data-direction'], 'asc');
element('installed-sort-name').onclick();
assert.equal(installedKeys(), 'a,b,d,c');
element('installed-sort-name').onclick();
assert.equal(installedKeys(), 'c,d,b,a');
assert.equal(element('installed-sort-name').attributes['data-direction'], 'desc');
element('installed-sort-source').onclick();
assert.equal(installedKeys(), 'd,a,b,c');
element('installed-sort-source').onclick();
assert.equal(installedKeys(), 'b,a,d,c', 'Unknown source stays last in either direction');
element('installed-filter').value = 'Ada';
assert.equal(installedKeys(), 'a');
element('installed-filter').value = '';
element('installed-sort-status').onclick();
assert.equal(installedKeys(), 'a,b,d,c');
element('installed-sort-status').onclick();
assert.equal(installedKeys(), 'd,b,a,c');
element('installed-sort-status').onclick();
assert.match(element('installed-sort-status').attributes['aria-label'], /highest priority first/);
// CurseForge rows: installable, opted-out (link only), already installed; status line and paging button.
run(`settings = { has_curse: true };
curse.rows = [
  {id:1,name:'TomTom <x>',summary:'Nav',authors:'A',category:'Map & Minimap',downloads:1500,url:'https://www.curseforge.com/projects/1',installable:true,installed:false,catalog_id:null},
  {id:2,name:'Leatrix',summary:'QoL',authors:'B',category:'',downloads:70000000,url:'u',installable:false,link_reason:'author',installed:false,catalog_id:null},
  {id:3,name:'Questie',summary:'Q',authors:'C',category:'Quests',downloads:5,url:'u',installable:true,installed:true,catalog_id:null},
  {id:4,name:'RetailOnly',summary:'R',authors:'D',category:'',downloads:9,url:'u',installable:false,link_reason:'no-forever',installed:false,catalog_id:null}
]; curse.query = 'tom'; curse.total = 120; curse.loaded = 50; renderCurseRows();`);
assert.match(element('curse-list').innerHTML, /TomTom &lt;x&gt;/);
assert.match(element('curse-list').innerHTML, /data-act="curse-install"/);
// A link-only row says why on the row itself, not only on hover, and an
// author's opt-out is never put down to the key.
const curseRow = (id) => element('curse-list').innerHTML.split('<div class="item"').find((r) => r.includes(`data-id="${id}"`));
assert.match(curseRow(2), />CurseForge app only<\/span>/);
assert.match(curseRow(2), /title="Its author lets only CurseForge&#39;s own app download it[^"]*An API key can&#39;t change that/);
assert.doesNotMatch(curseRow(2), /curse-install/);
assert.match(curseRow(4), />no Forever file<\/span>/);
assert.doesNotMatch(curseRow(4), /own app/, 'no Forever file is not the author opting out');
assert.match(element('curse-list').innerHTML, /installed/);
assert.match(element('curse-list').innerHTML, /Map &amp; Minimap/);
assert.equal(String(element('curse-status').textContent), '4 of 120 from CurseForge');
run(`curse.rows = []; curse.total = 0; curse.loaded = 0; curse.query = 'zzz'; renderCurseRows();`);
assert.equal(String(element('curse-status').textContent), 'Nothing on CurseForge for "zzz".');
assert.equal(element('curse-list').innerHTML, '');
// Type filter maps catalogue categories; unknown types match nothing special.
assert.equal(run("catalogMatchesType({category:'Bags'}, typeFor('bags'))"), true);
assert.equal(run("catalogMatchesType({category:'UI'}, typeFor('bags'))"), false);
assert.equal(run("catalogMatchesType({category:'UI'}, typeFor(''))"), true);
assert.equal(run("typeFor('pvp').curse"), 1004);
run(`settings = null; curse.rows = []; curse.query = '';`);
// Repeated heading clicks reverse direction; unknown counts stay below real zeroes.
run(`catalog = [
  {id:'z',name:'Zulu',category:'Utility',downloads:100,github:'example/z',updated_at:'2026-10-01'},
  {id:'a',name:'Alpha',category:'Bags',downloads:10,curse:1,updated_at:'2026-11-01'},
  {id:'b',name:'Beta',category:'Bags',downloads:10,github:'example/b',updated_at:'2026-11-02'},
  {id:'zero',name:'Zero',category:'UI',downloads:0,wowi:4,updated_at:''},
  {id:'unknown',name:'Unknown',category:'',downloads:null,updated_at:null}
];`);
const sortedNames = () => run('sortedCatalog([...catalog]).map(e => e.name).join()');
element('sort-downloads').onclick();
assert.equal(element('browse-sort').value, 'downloads');
assert.equal(sortedNames(), 'Zulu,Alpha,Beta,Zero,Unknown');
assert.equal(element('sort-downloads').attributes['data-direction'], 'desc');
element('sort-downloads').onclick();
assert.equal(element('browse-sort').value, 'downloads-asc');
assert.equal(sortedNames(), 'Zero,Alpha,Beta,Zulu,Unknown');
assert.equal(element('sort-downloads').attributes['data-direction'], 'asc');
assert.match(element('sort-downloads').attributes['aria-label'], /lowest to highest/);
element('sort-downloads').onclick();
assert.equal(element('browse-sort').value, 'downloads');
element('sort-name').onclick();
assert.equal(sortedNames(), 'Alpha,Beta,Unknown,Zero,Zulu');
assert.equal(element('sort-downloads').attributes['data-direction'], undefined);
element('sort-name').onclick();
assert.equal(sortedNames(), 'Zulu,Zero,Unknown,Beta,Alpha');
element('sort-category').onclick();
assert.equal(sortedNames(), 'Unknown,Alpha,Beta,Zero,Zulu');
element('sort-category').onclick();
assert.equal(sortedNames(), 'Zulu,Zero,Alpha,Beta,Unknown');
element('sort-source').onclick();
assert.equal(sortedNames(), 'Alpha,Unknown,Beta,Zulu,Zero');
element('sort-source').onclick();
assert.equal(sortedNames(), 'Zero,Beta,Zulu,Alpha,Unknown');
element('browse-sort').value = 'updated-asc';
element('browse-sort').listeners.change();
assert.equal(sortedNames(), 'Zulu,Alpha,Beta,Unknown,Zero');
assert.ok(sortHeads.every(head => head.attributes['data-direction'] === undefined));
element('browse-sort').value = 'name-desc';
element('browse-sort').listeners.change();
assert.equal(element('sort-name').attributes['data-direction'], 'desc');
element('browse-sort').value = 'downloads-asc';
const ascendingKey = run('curseParams().key');
assert.equal(run('curseParams().sort'), 'downloads-asc');
element('browse-sort').value = 'downloads';
assert.notEqual(run('curseParams().key'), ascendingKey);
// Unsupported server sorts keep the same alphabetical page in either direction.
for (const order of ['category', 'category-desc', 'source', 'source-desc']) {
  element('browse-sort').value = order;
  assert.equal(run('curseParams().sort'), 'name');
}
run(`settings = {has_curse:true}; curse.rows = [
  {id:1,name:'Zulu',category:'Bags',downloads:5},
  {id:2,name:'Alpha',category:'UI',downloads:2}
]; curse.total = 100; curse.loaded = 50; curse.loading = false;`);
element('browse-sort').value = 'source-desc';
run('renderCurseRows()');
assert.ok(element('curse-list').innerHTML.indexOf('Alpha') < element('curse-list').innerHTML.indexOf('Zulu'));
element('browse-sort').value = 'category-desc';
run('renderCurseRows()');
assert.match(element('curse-status').textContent, /Category order applies to shown results/);
run('curse.loaded = curse.total; renderCurseRows()');
assert.equal(element('curse-status').textContent, 'Category order applies to shown results');
run(`settings = null; curse.rows = []; curse.total = 0; curse.loaded = 0;`);
element('browse-sort').value = 'name';
// Source picker: one source is a link, several become a select; Wago ids and locked CurseForge ids never count.
run(`settings = { has_curse: true };
catalog = [
  { id:'one', name:'One', github:'x/one' },
  { id:'two', name:'Two', github:'x/two', curse: 7, wago: 'w' },
  { id:'locked', name:'Locked', curse: 8, curse_locked: true },
  { id:'wagoonly', name:'WagoOnly', wago: 'w2' },
];
document.getElementById('browse-filter').value = ''; document.getElementById('browse-cat').value = ''; renderCatalog();`);
assert.equal(run("installSources(catalog[0]).length"), 1);
assert.equal(run("installSources(catalog[1]).map(s => s.kind).join()"), 'github,curse', 'a Wago id never makes a source');
assert.equal(run("installSources(catalog[2]).length"), 0);
assert.equal(run("installSources(catalog[3]).length"), 0, 'Wago-only is link only');
assert.match(element('browse-list').innerHTML, /<select class="src-pick"[^>]*><option value="github">GitHub/);
const catalogRow = (id) => element('browse-list').innerHTML.split('<div class="item"').find((r) => r.includes(`data-id="${id}"`));
assert.match(catalogRow('locked'), />CurseForge app only<\/span>/, 'an opted-out author is named as the reason');
assert.match(catalogRow('wagoonly'), />Wago only<\/span>/);
assert.doesNotMatch(element('browse-list').innerHTML, /Wago key/);
// The same reasons reach the details pane, and without a key a CurseForge-only entry asks for one.
assert.match(run("linkOnlyWhy(catalog[2]).tip"), /^Its author lets only CurseForge's own app/);
assert.notEqual(run("linkOnlyWhy({ curse: 9 }).label"), 'needs API key', 'with a key, never ask for one');
run(`settings = { has_curse: false }`);
assert.equal(run("linkOnlyWhy({ curse: 9 }).label"), 'needs API key');
assert.equal(run("linkOnlyWhy({ curse: 9, curse_locked: true }).label"), 'CurseForge app only', 'a key would not help an opted-out author');
assert.equal(run("linkOnlyWhy({}).label"), 'link only');
run(`settings = { has_curse: true }`);
run(`renderImportPlan([{ name: 'List <b>', key: 'x', installed: false, installable: true, source: { kind: 'github', id: 'a/<img src=x>' }, why: '' }])`);
assert.match(element('import-plan').innerHTML, /a\/&lt;img src=x&gt;/, 'An imported source is shown as text');
assert.doesNotMatch(element('import-plan').innerHTML, /<img/, 'An imported list cannot add markup');
run(`settings = null; catalog = [];`);
run(`packages = []; renderInstalled()`);
assert.equal(String(element('stat-updates').textContent), '0');
assert.match(element('installed-list').innerHTML, /Discover addons/);
const menu = { style: {}, offsetHeight: 110 };
const details = {
  open: true, matches: () => true, querySelector: () => menu,
  getBoundingClientRect: () => ({ top: 580, bottom: 612, right: 920 }),
};
context.window.innerWidth = 980;
context.window.innerHeight = 660;
element('installed-list').listeners.toggle({ target: details });
assert.ok(parseFloat(menu.style.top) >= 0);
assert.ok(parseFloat(menu.style.top) + menu.offsetHeight < 580, 'Bottom-row options open above the trigger');
assert.ok(parseFloat(menu.style.right) >= 12, 'Options stay inside the right edge');
(async () => {
  const originalFocus = document.activeElement;
  const promise = run(`confirmModal('Confirm?', 'Remove')`);
  assert.equal(document.activeElement, element('modal-no'));
  element('modal-no').onclick();
  assert.equal(await promise, false);
  assert.equal(document.activeElement, originalFocus);
  run(`catalog = [{ id:'boss', name:'Boss', github:'example/boss', desc:'Boss timers', installed:true }];`);
  detailReply = Promise.resolve({summary:'Release description', image:'data:image/png;base64,test'});
  await run(`openDetail('boss', {key:'boss-local',name:'Boss',folders:['Boss'],installed_version:'2',remote_version:'3',remote_prerelease:true})`);
  assert.equal(element('detail-desc').textContent, 'Boss timers');
  assert.match(element('detail-meta').innerHTML, /Installed 2.*Available 3 \(beta\)/);
  assert.match(element('detail-body').innerHTML, /Release description/);
  assert.match(element('detail-body').innerHTML, /class="shot"/);
  run('closeDetail()');
  assert.equal(document.activeElement, originalFocus);
  await run(`openDetail(null, {key:'local',name:'Local addon',folders:['Local<folder>'],notes:'Local description',source_url:'https://example.test/addon'})`);
  assert.equal(element('detail-desc').textContent, 'Local description');
  assert.match(element('detail-body').innerHTML, /Local&lt;folder&gt;/);
  assert.equal(run('entryUrl(detailEntry)'), 'https://example.test/addon');
  run('closeDetail()');
  let finish;
  detailReply = new Promise(resolve => { finish = resolve; });
  const loading = run("openDetail('boss')");
  run('closeDetail()');
  const before = element('detail-body').innerHTML;
  finish({summary:'Late response'});
  await loading;
  assert.equal(element('detail-body').innerHTML, before, 'Closed dialogs ignore late responses');

  // Actionable updates move up without changing the order within each group.
  const addon = (key, name, status, flags = {}) => ({
    key, name, status, folders: [name], source: true, source_label: 'GitHub',
    supports_forever: true, missing_deps: [], installed_version: '1', ...flags,
  });
  const batch = [
    addon('normal', 'Alpha', 'ok'),
    addon('pinned', 'Bravo', 'update', { pinned: true, remote_version: '2' }),
    addon('updated', 'Charlie', 'update'),
    addon('ignored', 'Delta', 'update', { ignored: true, remote_version: '2' }),
    addon('failed', 'Zulu', 'update'),
  ];
  context.batch = batch.map(p => ({ ...p }));
  element('show-ignored').checked = false;
  run(`activeFilter = 'all'; packages = batch; renderInstalled();`);
  assert.equal(run(`visiblePackages().map(p => p.key).join()`), 'updated,failed,normal,pinned');
  const batchStart = invokeCalls.length;
  invokeReply = (command, args) => {
    if (command === 'update_package') {
      if (args.key === 'failed') return Promise.reject('cannot replace addon folder QuestieDB: it belongs to folder:QuestieDB');
      return Promise.resolve({ ...batch[2], status: 'ok', installed_version: '2', remote_version: '2' });
    }
    if (command === 'scan') return Promise.resolve(batch.map(p => ({ ...p, status: 'unknown', installed_version: p.key === 'updated' ? '2' : '1' })));
    return Promise.resolve({});
  };
  await run('updateAll()');
  assert.deepEqual(invokeCalls.slice(batchStart).filter(c => c.command === 'update_package').map(c => c.args.key), ['updated', 'failed']);
  assert.ok(invokeCalls.slice(batchStart).some(c => c.command === 'scan'), 'A successful batch item refreshes related rows');
  assert.equal(installedKeys(), 'failed,normal,updated,pinned');
  assert.match(element('toast').textContent, /1 updated, 1 failed \(Zulu\)/);
  assert.match(element('installed-list').innerHTML, /Update failed/);
  assert.match(element('installed-list').innerHTML, /The QuestieDB folder is treated as a separate addon/);
  assert.doesNotMatch(element('installed-list').innerHTML, /folder:QuestieDB/);
  element('show-ignored').checked = true;
  assert.equal(installedKeys(), 'failed,normal,updated,pinned,ignored');
  element('show-ignored').checked = false;
  assert.equal(run(`activeFilter = 'updates'; visiblePackages().map(p => p.key).join()`), 'failed,pinned', 'Updates filter keeps the failed row and the pinned update after refresh');
  // Update all runs several at once: every update starts before the first one finishes.
  const pending = [];
  const parallelBatch = ['a', 'b', 'c', 'd', 'e'].map(k => addon(k, k.toUpperCase(), 'update'));
  context.parallelBatch = parallelBatch;
  run(`packages = parallelBatch.map(p => ({ ...p }))`);
  const parallelStart = invokeCalls.length;
  invokeReply = (command, args) => {
    if (command === 'update_package') return new Promise(resolve => pending.push(() => resolve({ ...parallelBatch.find(p => p.key === args.key), status: 'ok', installed_version: '2' })));
    if (command === 'scan') return Promise.resolve(parallelBatch.map(p => ({ ...p, status: 'ok', installed_version: '2' })));
    return Promise.resolve({});
  };
  const parallelRun = run('updateAll()');
  await new Promise(r => setTimeout(r, 0));
  assert.equal(invokeCalls.slice(parallelStart).filter(c => c.command === 'update_package').length, 4, 'Four updates start together');
  while (pending.length) { pending.shift()(); await new Promise(r => setTimeout(r, 0)); }
  await parallelRun;
  assert.deepEqual(invokeCalls.slice(parallelStart).filter(c => c.command === 'update_package').map(c => c.args.key), ['a', 'b', 'c', 'd', 'e']);
  assert.match(element('toast').textContent, /^5 updated/);
  run(`packages = batch; renderInstalled();`);
  invokeReply = () => Promise.resolve({ ...batch[4], status: 'ok', installed_version: '2' });
  await run(`updateOne(packages.find(p => p.key === 'failed'))`);
  assert.equal(run(`visiblePackages().some(p => p.key === 'failed')`), false, 'Successful retry clears the failure');
  run(`activeFilter = 'all'; packages = []; renderInstalled();`);

  // Reversing remote results restarts pagination, keeps its direction on more,
  // and ignores responses that arrive from the previous direction.
  run(`settings = {has_curse:true}; curse.key = ''; curse.query = ''; curse.loaded = 50; curse.rows = [];`);
  element('browse-sort').value = 'downloads-asc';
  invokeReply = () => Promise.resolve({rows:[{id:10,name:'First',downloads:0}],total:100});
  await run('loadCurse(false)');
  assert.equal(invokeCalls.at(-1).args.sort, 'downloads-asc');
  assert.equal(invokeCalls.at(-1).args.index, 0);
  await run('loadCurse(true)');
  assert.equal(invokeCalls.at(-1).args.sort, 'downloads-asc');
  assert.equal(invokeCalls.at(-1).args.index, 1);
  let finishPreviousSort;
  element('browse-sort').value = 'downloads';
  invokeReply = () => new Promise(resolve => { finishPreviousSort = resolve; });
  const previousSort = run('loadCurse(false)');
  element('browse-sort').value = 'downloads-asc';
  invokeReply = () => Promise.resolve({rows:[{id:20,name:'Latest',downloads:1}],total:100});
  await run('loadCurse(false)');
  finishPreviousSort({rows:[{id:30,name:'Stale',downloads:100}],total:100});
  await previousSort;
  assert.equal(run('curse.rows.map(r => r.id).join()'), '20');
  run(`settings = null; curse.rows = [];`);
  element('browse-sort').value = 'name';

  // Exercise the actual settings handlers, including pending and failed calls.
  run('settings = { auto_update: false, allow_prerelease: true };');
  element('pref-autoupdate').checked = true;
  let finishPreference;
  invokeReply = () => new Promise(resolve => { finishPreference = resolve; });
  const savingPreference = element('pref-autoupdate').onchange();
  assert.equal(element('pref-autoupdate').disabled, true, 'Preference waits for persistence');
  assert.equal(invokeCalls.at(-1).command, 'set_prefs');
  assert.equal(invokeCalls.at(-1).args.autoUpdate, true);
  assert.deepEqual(Object.keys(invokeCalls.at(-1).args), ['autoUpdate'], 'Only the changed preference is sent');
  finishPreference();
  await savingPreference;
  assert.equal(run('settings.auto_update'), true);
  assert.equal(element('pref-autoupdate').disabled, false);

  element('pref-autoupdate').checked = false;
  invokeReply = () => Promise.reject(new Error('Preference write failed'));
  await element('pref-autoupdate').onchange();
  assert.equal(element('pref-autoupdate').checked, true, 'Failed save restores the saved checkbox value');
  assert.equal(run('settings.auto_update'), true, 'Failed save does not change the saved preference');
  assert.equal(element('pref-autoupdate').disabled, false);
  assert.match(element('toast').textContent, /Preference write failed/);
  assert.match(element('toast').className, /bad/);

  element('pref-prerelease').checked = false;
  invokeReply = () => Promise.resolve();
  await element('pref-prerelease').onchange();
  assert.equal(invokeCalls.at(-1).command, 'set_prefs');
  assert.equal(invokeCalls.at(-1).args.allowPrerelease, false);
  assert.deepEqual(Object.keys(invokeCalls.at(-1).args), ['allowPrerelease']);
  assert.equal(run('settings.allow_prerelease'), false);
  invokeReply = () => Promise.resolve({ version: '0.4.0', build: 28, auto_update: true, allow_prerelease: false });
  await run('loadSettings()');
  assert.equal(element('pref-autoupdate').checked, true, 'Saved auto-update preference survives settings reload');
  assert.equal(element('pref-prerelease').checked, false);

  element('btn-backup-wtf').textContent = 'Back up the WTF folder';
  let finishBackup;
  invokeReply = () => new Promise(resolve => { finishBackup = resolve; });
  const startingBackup = element('btn-backup-wtf').onclick();
  assert.equal(invokeCalls.at(-1).command, 'backup_wtf');
  assert.equal(element('btn-backup-wtf').disabled, true);
  assert.equal(element('btn-backup-wtf').textContent, 'Backing up…');
  const callsWhileBackingUp = invokeCalls.length;
  await element('btn-backup-wtf').onclick();
  assert.equal(invokeCalls.length, callsWhileBackingUp, 'A second click cannot start a second backup');
  finishBackup({ files: 2, bytes: 100, path: 'QA/WTF-synthetic.zip' });
  await startingBackup;
  assert.match(element('toast').textContent, /Backed up 2 files to QA\/WTF-synthetic\.zip/);
  assert.equal(element('btn-backup-wtf').disabled, false);
  assert.equal(element('btn-backup-wtf').textContent, 'Back up the WTF folder');

  invokeReply = () => Promise.reject(new Error('No WTF folder in the game folder yet'));
  await element('btn-backup-wtf').onclick();
  assert.match(element('toast').textContent, /No WTF folder/);
  assert.match(element('toast').className, /bad/);
  assert.equal(element('btn-backup-wtf').disabled, false);
  assert.equal(run('busy'), false, 'A failed backup releases the interface');
  assert.equal(element('btn-backup-wtf').textContent, 'Back up the WTF folder');

  invokeReply = () => Promise.resolve('QA/backups');
  await element('btn-open-backups').onclick();
  assert.equal(invokeCalls.at(-1).command, 'open_backups_folder');
  invokeReply = () => Promise.reject(new Error('Could not open backups folder'));
  await element('btn-open-backups').onclick();
  assert.match(element('toast').textContent, /Could not open backups folder/);
  assert.match(element('toast').className, /bad/);
  // A CurseForge-only row: one line, and an update you have to fetch yourself.
  run(`packages = [
    {key:'bt',name:'Bartender4',folders:['Bartender4'],source_label:'CurseForge',source_url:'https://www.curseforge.com/projects/1',status:'curse-update',supports_forever:true,missing_deps:[],installed_version:'4.15.3',remote_version:'Bartender4-4.16.0',note:'Bartender4-4.16.0 is on CurseForge; you have 4.15.3. Open its page to download it, then use Install from zip.'},
    {key:'bg',name:'Bagnon',folders:['Bagnon'],source_label:'CurseForge',status:'curse-only',supports_forever:true,missing_deps:[],note:'The author only distributes this through CurseForge.'}
  ]; activeFilter = 'all'; $('installed-filter').value = ''; $('show-ignored').checked = true; renderInstalled();`);
  assert.match(element('installed-list').innerHTML, /Update on CurseForge/);
  assert.match(element('summary').textContent, /1 to get from CurseForge/);
  assert.equal(String(element('stat-curse').textContent), '1');
  assert.equal(String(element('stat-updates').textContent), '0', 'Update all never counts one it cannot install');
  assert.equal(element('btn-update-all').textContent, 'Update all');
  assert.equal(String(element('stat-attention').textContent), '1');
  assert.equal((element('installed-list').innerHTML.match(/class="note/g) || []).length, 2, 'One line per CurseForge row');
  assert.doesNotMatch(element('installed-list').innerHTML, /data-act="update"/, 'Never an Install or Update button for a row we cannot download');
  run(`activeFilter = 'updates'; renderInstalled();`);
  assert.match(element('installed-list').innerHTML, /Bartender4/, 'The Updates view lists it');
  assert.doesNotMatch(element('installed-list').innerHTML, /Bagnon/);
  run(`activeFilter = 'all';`);

  // Remove for an addon AddonForge did not install asks a different question.
  run(`packages = [{key:'h',name:'HealBot',folders:['HealBot'],source_label:'',status:'no-source',supports_forever:true,missing_deps:[],managed:false,note:'No update source found in its files'}];
    settings = { recycle_label: 'Recycle Bin' }; renderInstalled();`);
  const rowOf = (key) => ({ dataset: { key } });
  const clickAction = (key, act) => {
    const button = { dataset: { act }, closest: (sel) => (sel === '.item' ? rowOf(key) : null) };
    return element('installed-list').listeners.click({ target: { closest: (sel) => (sel === 'button[data-act]' ? button : null) } });
  };
  invokeReply = (command) => Promise.resolve(command === 'remove_package' ? ['HealBot'] : []);
  const removing = clickAction('h', 'remove');
  assert.match(element('modal-text').innerHTML, /didn&#39;t install|didn't install/);
  assert.match(element('modal-text').innerHTML, /Recycle Bin/);
  assert.equal(element('modal-yes').textContent, 'Move to Recycle Bin');
  element('modal-yes').onclick();
  await removing;
  assert.equal(JSON.stringify(invokeCalls.find((c) => c.command === 'remove_package').args), '{"key":"h","recycle":true}');
  assert.match(element('toast').textContent, /Moved HealBot to the Recycle Bin/);

  // An addon it did install keeps the plain delete wording.
  run(`packages = [{key:'m',name:'Mine',folders:['Mine'],source_label:'GitHub',status:'ok',supports_forever:true,missing_deps:[],managed:true}]; renderInstalled();`);
  const removingMine = clickAction('m', 'remove');
  assert.match(element('modal-text').innerHTML, /Remove <b>Mine<\/b>/);
  assert.equal(element('modal-yes').textContent, 'Remove');
  element('modal-yes').onclick();
  await removingMine;
  assert.match(element('toast').textContent, /Removed Mine/);

  // CurseForge API key. The state line says whose key is in use; the key itself
  // never comes back from the core, and a saved one does not stay on screen.
  run(`settings = { has_curse: false, curse_key: 'none', secret_store: 'Windows Credential Manager', curse_key_page: 'https://console.curseforge.com/' };
    packages = []; curse.key = ''; curse.rows = []; renderCurseKey();`);
  assert.match(element('curse-key-state').textContent, /No CurseForge API key yet/);
  assert.equal(element('secret-store').textContent, 'Windows Credential Manager');
  assert.equal(element('curse-key-page').dataset.url, 'https://console.curseforge.com/');
  assert.match(element('browse-filter').placeholder, /Search the catalogue/);
  assert.match(run(`curseKeyStatus({ curse_key: 'none', secret_store: null })`), /can't be opened/);
  assert.match(run(`curseKeyStatus({ curse_key: 'embedded', secret_store: 'x' })`), /This build carries/);
  assert.match(run(`curseKeyStatus(null)`), /No CurseForge API key/);

  // A key CurseForge refuses changes nothing and stays in the box to be corrected.
  element('curse-key').value = 'typed-key-that-is-refused';
  invokeReply = (command) => command === 'set_curse_key' ? Promise.reject('CurseForge did not accept that key for World of Warcraft.') : Promise.resolve([]);
  await element('btn-curse-key-save').onclick();
  assert.equal(invokeCalls.at(-1).command, 'set_curse_key');
  assert.equal(JSON.stringify(invokeCalls.at(-1).args), '{"key":"typed-key-that-is-refused"}');
  assert.match(element('toast').textContent, /did not accept/);
  assert.match(element('toast').className, /bad/);
  assert.equal(element('curse-key').value, 'typed-key-that-is-refused');
  assert.equal(run('settings.has_curse'), false);
  assert.equal(run('busy'), false, 'A refused key releases the interface');
  assert.equal(element('btn-curse-key-save').textContent, 'Save and test');

  // An accepted key turns CurseForge on at once and is cleared from the box.
  element('curse-key').value = 'typed-key-that-is-accepted';
  invokeReply = (command) => Promise.resolve(command === 'set_curse_key' ? { has_curse: true, curse_key: 'user' } : command === 'curse_search' ? { rows: [], total: 0 } : []);
  await element('btn-curse-key-save').onclick();
  assert.equal(element('curse-key').value, '', 'A saved key does not stay on screen');
  assert.equal(run('settings.has_curse'), true);
  assert.equal(run('settings.curse_key'), 'user');
  assert.match(element('curse-key-state').textContent, /saved in Windows Credential Manager/);
  assert.match(element('browse-filter').placeholder, /Search addons/);
  assert.match(element('curse-key').placeholder, /replace the saved one/);
  assert.doesNotMatch(element('toast').textContent + element('curse-key-state').textContent, /typed-key/, 'The key is never echoed back');
  assert.equal(invokeCalls.at(-1).command, 'curse_search', 'CurseForge rows load as soon as there is a key');

  // Removing asks first; cancelling leaves the key alone.
  const callsBeforeCancel = invokeCalls.length;
  const cancelledRemoval = element('btn-curse-key-clear').onclick();
  assert.match(element('modal-text').innerHTML, /Remove your CurseForge API key/);
  assert.equal(element('modal-yes').textContent, 'Remove key');
  element('modal-no').onclick();
  await cancelledRemoval;
  assert.equal(invokeCalls.length, callsBeforeCancel, 'Cancel removes nothing');
  assert.equal(run('settings.curse_key'), 'user');
  invokeReply = (command) => Promise.resolve(command === 'clear_curse_key' ? { has_curse: false, curse_key: 'none' } : []);
  const removingKey = element('btn-curse-key-clear').onclick();
  element('modal-yes').onclick();
  await removingKey;
  assert.ok(invokeCalls.slice(callsBeforeCancel).some((c) => c.command === 'clear_curse_key'));
  assert.equal(run('settings.has_curse'), false);
  assert.match(element('curse-key-state').textContent, /No CurseForge API key yet/);
  assert.match(element('toast').textContent, /removed from this PC/);
  assert.equal(run('curse.rows.length'), 0, 'CurseForge rows go when the key goes');

  // Self-update. A build with no update channel offers nothing and does not
  // claim to be up to date; a build with one shows the banner as before.
  let bannerHidden = null;
  element('selfupdate').classList = { add: () => { bannerHidden = true; }, remove: () => { bannerHidden = false; }, toggle() {} };
  invokeReply = () => Promise.resolve({ enabled: false, available: false, latest: null, tag: '', error: null });
  element('toast').textContent = '';
  await run('checkSelf(false)');
  assert.equal(invokeCalls.at(-1).command, 'check_self_update');
  assert.equal(bannerHidden, true, 'No banner without an update channel');
  assert.equal(element('toast').textContent, '', 'The launch check says nothing');
  await run('checkSelf(true)');
  assert.match(element('toast').textContent, /does not update itself/);
  assert.doesNotMatch(element('toast').textContent, /up to date/);
  // Even a reply that claims an update is ignored when the channel is off.
  invokeReply = () => Promise.resolve({ enabled: false, available: true, latest: { version: '9.9.9', build: 99 }, tag: 'v9.9.9-b99', error: null });
  bannerHidden = null;
  await run('checkSelf(false)');
  assert.equal(bannerHidden, true, 'An offer is never shown when the channel is off');
  invokeReply = () => Promise.resolve({ enabled: true, available: true, latest: { version: '9.9.9', build: 99 }, tag: 'v9.9.9-b99', error: null });
  await run('checkSelf(false)');
  assert.equal(bannerHidden, false, 'A build with its own channel still offers its update');
  assert.match(element('selfupdate-text').innerHTML, /v9\.9\.9<\/b> build 99/);
  invokeReply = () => Promise.resolve({ enabled: true, available: false, latest: null, tag: '', error: null });
  await run('checkSelf(true)');
  assert.match(element('toast').textContent, /up to date/);

  // Settings swap the "check now" link for a plain statement when there is no channel.
  const shown = {};
  for (const id of ['btn-selfcheck', 'selfupdate-off']) element(id).classList = { add() {}, remove() {}, toggle: (cls, on) => { if (cls === 'hidden') shown[id] = !on; } };
  invokeReply = () => Promise.resolve({ version: '0.4.0', build: 0, self_update: false });
  await run('loadSettings()');
  assert.equal(shown['btn-selfcheck'], false);
  assert.equal(shown['selfupdate-off'], true);
  invokeReply = () => Promise.resolve({ version: '0.4.0', build: 0, self_update: true });
  await run('loadSettings()');
  assert.equal(shown['btn-selfcheck'], true);
  assert.equal(shown['selfupdate-off'], false);

  // Install from a zip on disk. The dialog offers zips only, the core does the
  // install, and the row says where it came from and never offers an update.
  const zipRow = {
    key: 'curse:990001', name: 'NetherSuite', folders: ['NetherSuite'], source: null, source_label: 'Zip file',
    source_url: 'https://www.curseforge.com/projects/990001', from_zip: 'NetherSuite-1.2.3.zip', curse_id: 990001,
    catalog_id: null, status: 'local', managed: true, supports_forever: true, missing_deps: [], installed_version: '1.2.3',
    note: "Installed from a zip file (NetherSuite-1.2.3.zip). AddonForge doesn't update it: install a newer zip to replace it",
  };
  context.zipRow = zipRow;
  run(`settings = { recycle_label: 'Recycle Bin' }; packages = []; catalog = []; curse.rows = [{ id: 990001, name: 'NetherSuite', installed: false, installable: false, link_reason: 'author' }];
    activeFilter = 'all'; $('installed-filter').value = ''; $('show-ignored').checked = true; busy = false;`);
  const zipCalls = () => invokeCalls.filter((c) => c.command === 'install_zip').map((c) => c.args);
  const zipAnswer = (command, args) => command === 'install_zip'
    ? Promise.resolve({ ...zipRow, key: 'k:' + args.path, name: args.path.split(/[\\/]/).pop().replace(/\.zip$/i, '') })
    : command === 'scan' ? Promise.resolve([]) : Promise.resolve([]);
  invokeReply = (command, args) => command === 'install_zip' ? Promise.resolve({ ...zipRow }) : command === 'scan' ? Promise.resolve([{ ...zipRow, status: 'unknown' }]) : Promise.resolve([]);
  dialogReply = () => Promise.resolve(['C:\\Users\\x\\Downloads\\NetherSuite-1.2.3.zip']);
  await element('btn-install-zip').onclick();
  assert.deepEqual(JSON.parse(JSON.stringify(dialogCalls.at(-1).filters)), [{ name: 'Zip archive', extensions: ['zip'] }]);
  assert.equal(dialogCalls.at(-1).directory, false);
  assert.deepEqual(JSON.parse(JSON.stringify(zipCalls())), [{ path: 'C:\\Users\\x\\Downloads\\NetherSuite-1.2.3.zip', allowNonForever: false }]);
  assert.match(element('toast').textContent, /^Installed NetherSuite 1\.2\.3 from a zip file\. AddonForge won't update it/);
  assert.doesNotMatch(element('toast').className, /bad/);
  assert.equal(run('busy'), false, 'An install from a zip releases the interface');
  assert.equal(run('curse.rows[0].installed'), true, 'Its CurseForge row now says installed');
  let zipHtml = element('installed-list').innerHTML;
  assert.match(zipHtml, /From a zip file/, 'The row keeps the status the install returned, not the rescan\'s');
  assert.match(zipHtml, /title="Installed from NetherSuite-1\.2\.3\.zip\. The link opens its page">Zip file<\/a>/);
  assert.match(zipHtml, /doesn&#39;t update it/);
  assert.doesNotMatch(zipHtml, /data-act="update"/, 'A zip install is never offered an update or a reinstall');
  assert.match(zipHtml, /data-act="pin"/);
  assert.match(zipHtml, /data-act="remove"/);
  assert.equal(String(element('stat-attention').textContent), '0', 'A zip install is not a problem to look at');
  // A newer release on CurseForge is reported and still not offered.
  run(`packages = [{ ...zipRow, status: 'curse-update', remote_version: 'NetherSuite-1.3.0', note: 'NetherSuite-1.3.0 is on CurseForge; you have 1.2.3. Open its page to download it, then use Install from zip.' }]; renderInstalled();`);
  zipHtml = element('installed-list').innerHTML;
  assert.match(zipHtml, /Update on CurseForge/);
  assert.match(zipHtml, /then use Install from zip/);
  assert.doesNotMatch(zipHtml, /data-act="update"/);
  assert.equal(String(element('stat-curse').textContent), '1');
  // A cancelled dialog does nothing.
  const beforeCancel = invokeCalls.length;
  dialogReply = () => Promise.resolve(null);
  await element('btn-install-zip').onclick();
  assert.equal(invokeCalls.length, beforeCancel);
  // No Forever TOC: asked first, as for a download; "Install anyway" sends it again with consent.
  invokeReply = (command, args) => command === 'install_zip'
    ? (args.allowNonForever ? Promise.resolve({ ...zipRow, name: 'Classic', installed_version: null }) : Promise.reject('NOT_FOREVER'))
    : Promise.resolve([]);
  const asking = run(`installZips(['D:/zips/Classic.zip'])`);
  await new Promise((r) => setTimeout(r, 0));
  assert.match(element('modal-text').innerHTML, /<b>Classic\.zip<\/b> has no Forever TOC/);
  element('modal-yes').onclick();
  await asking;
  assert.deepEqual(zipCalls().slice(-2).map((a) => a.allowNonForever), [false, true]);
  assert.match(element('toast').textContent, /^Installed Classic from a zip file/);
  // Declining installs nothing and says nothing more.
  element('toast').textContent = '';
  const declining = run(`installZips(['D:/zips/Classic.zip'])`);
  await new Promise((r) => setTimeout(r, 0));
  element('modal-no').onclick();
  await declining;
  assert.equal(zipCalls().at(-1).allowNonForever, false);
  assert.equal(element('toast').textContent, '');
  // A zip the core refuses says why, in the app's words.
  invokeReply = (command) => command === 'install_zip'
    ? Promise.reject('SAFETY: refused, the zip contains an executable file (Evil/helper.exe) which no WoW addon needs')
    : Promise.resolve([]);
  await run(`installZips(['D:/zips/Evil.zip'])`);
  assert.match(element('toast').textContent, /^Not installed: Evil\.zip: Refused for safety: the zip contains an executable file/);
  assert.match(element('toast').className, /bad/);
  assert.equal(run('busy'), false);

  // Dragging files over the window: the overlay shows only when a zip is among them.
  let dropShown = null;
  element('drop-zone').classList = { add() {}, remove() {}, toggle: (cls, on) => { if (cls === 'hidden') dropShown = !on; } };
  for (const name of ['tauri://drag-enter', 'tauri://drag-leave', 'tauri://drag-drop']) assert.equal(typeof tauriListeners[name], 'function', name);
  tauriListeners['tauri://drag-enter']({ payload: { paths: ['C:/x/notes.txt'] } });
  assert.equal(dropShown, false);
  tauriListeners['tauri://drag-enter']({ payload: { paths: ['C:/x/notes.txt', 'C:/x/Pack.ZIP'] } });
  assert.equal(dropShown, true);
  tauriListeners['tauri://drag-leave']({ payload: null });
  assert.equal(dropShown, false);
  // A drop installs the zips, one after another, and leaves everything else alone.
  invokeReply = zipAnswer;
  const dropStart = zipCalls().length;
  dropShown = true;
  await tauriListeners['tauri://drag-drop']({ payload: { paths: ['C:/x/notes.txt', 'C:/x/Pack.ZIP', 'C:/x/Other.zip'], position: { x: 1, y: 1 } } });
  assert.equal(dropShown, false);
  assert.deepEqual(zipCalls().slice(dropStart).map((a) => a.path), ['C:/x/Pack.ZIP', 'C:/x/Other.zip']);
  assert.match(element('toast').textContent, /^Installed 2 addons from zip files\. AddonForge won't update them\. 1 file was not a \.zip and left alone/);
  assert.doesNotMatch(element('toast').className, /bad/);
  // Nothing to install: no call at all.
  await tauriListeners['tauri://drag-drop']({ payload: { paths: ['C:/x/notes.txt'] } });
  assert.equal(zipCalls().length, dropStart + 2);
  assert.match(element('toast').textContent, /Only \.zip files can be installed/);
  // While another job runs, a drop waits for the user to try again.
  run('busy = true');
  await tauriListeners['tauri://drag-drop']({ payload: { paths: ['C:/x/Pack.zip'] } });
  assert.equal(zipCalls().length, dropStart + 2);
  assert.match(element('toast').textContent, /busy/);
  run('busy = false');
  // An opted-out CurseForge row points at the same route.
  assert.match(run(`LINK_ONLY.author.tip`), /download the zip there yourself, then use Install from zip in My addons/);

  // Network settings. The switches show what the core saved, a change sends the
  // whole set, and a failed save puts the boxes back.
  const allOn = { offline: false, catalogue: true, stats: true, launch_check: true, images: true, curseforge: true, github: true, wowinterface: true, tukui: true };
  let badgeShown = null;
  element('offline-badge').classList = { add() {}, remove() {}, toggle: (cls, on) => { if (cls === 'hidden') badgeShown = !on; } };
  invokeReply = () => Promise.resolve({ version: '0.4.0', build: 0, network: { ...allOn, github: false }, catalog_count: 128, catalog_source: 'bundled (offline)', catalog_updated: '' });
  await run('loadSettings()');
  assert.equal(element('net-github').checked, false);
  assert.equal(element('net-curseforge').checked, true);
  assert.equal(element('net-offline').checked, false);
  assert.equal(badgeShown, false, 'No offline badge while online');
  assert.match(element('catalog-info').textContent, /built-in copy \(offline mode\)/);
  const netCalls = () => invokeCalls.filter((c) => c.command === 'set_network').map((c) => c.args.network);
  invokeReply = (command, args) => command === 'set_network' ? Promise.resolve(args.network)
    : command === 'network_activity' ? Promise.resolve({ policy: args, entries: [], counts: [] }) : Promise.resolve([]);
  element('net-offline').checked = true;
  await element('net-offline').onchange();
  assert.deepEqual(JSON.parse(JSON.stringify(netCalls().at(-1))), { ...allOn, github: false, offline: true }, 'The whole set is sent, the GitHub switch included');
  assert.equal(run('settings.network.offline'), true);
  assert.equal(badgeShown, true, 'The header says the app is offline');
  assert.match(element('toast').textContent, /^Offline mode is on/);
  assert.equal(invokeCalls.at(-1).command, 'network_activity', 'The activity list refreshes after a change');
  assert.equal(element('net-offline').disabled, false);
  // A save the core refuses changes nothing.
  element('net-tukui').checked = false;
  invokeReply = (command) => command === 'set_network' ? Promise.reject('settings file is locked') : Promise.resolve({ entries: [], counts: [] });
  await element('net-tukui').onchange();
  assert.equal(element('net-tukui').checked, true, 'A failed save restores the box');
  assert.equal(run('settings.network.tukui'), true);
  assert.match(element('toast').textContent, /settings file is locked/);
  assert.match(element('toast').className, /bad/);
  // Back online with the catalogue on: the core dropped its copy, so the catalogue reloads.
  invokeReply = (command, args) => command === 'set_network' ? Promise.resolve(args.network) : Promise.resolve(command === 'network_activity' ? { entries: [], counts: [] } : []);
  const beforeOnline = invokeCalls.length;
  element('net-offline').checked = false;
  await element('net-offline').onchange();
  assert.ok(invokeCalls.slice(beforeOnline).some((c) => c.command === 'catalog_list'), 'Leaving offline mode fetches the catalogue again');
  assert.equal(element('toast').textContent, 'Offline mode is off.');

  // The launch check: switched off or offline it asks nothing and says so.
  const checksBefore = () => invokeCalls.filter((c) => c.command === 'check_updates').length;
  let checks = checksBefore();
  run(`settings.network = { ...settings.network, launch_check: false }`);
  assert.equal(await run('launchCheck()'), false);
  assert.equal(checksBefore(), checks, 'No check when it is switched off');
  assert.match(element('last-checked').textContent, /Not checked at launch/);
  run(`settings.network = { ...settings.network, launch_check: true, offline: true }`);
  assert.equal(await run('launchCheck()'), false);
  assert.equal(checksBefore(), checks, 'No check in offline mode');
  assert.equal(element('last-checked').textContent, 'Offline mode: not checked');
  run(`settings.network = { ...settings.network, offline: false }`);
  invokeReply = (command) => Promise.resolve(command === 'check_updates' ? [] : []);
  assert.equal(await run('launchCheck()'), true);
  assert.equal(JSON.stringify(invokeCalls.filter((c) => c.command === 'check_updates').at(-1).args), '{"atLaunch":true}');
  await run('checkUpdates()');
  assert.equal(JSON.stringify(invokeCalls.at(-1).args), '{"atLaunch":false}', 'Asked for by hand, the check is not the launch check');
  // The Check button's click event never reaches the core as `atLaunch`.
  await element('btn-check').onclick({ type: 'click', target: element('btn-check') });
  assert.equal(invokeCalls.at(-1).command, 'check_updates');
  assert.equal(JSON.stringify(invokeCalls.at(-1).args), '{"atLaunch":false}', 'A click is a check by hand');
  await run(`checkUpdates({ type: 'click' })`);
  assert.equal(JSON.stringify(invokeCalls.at(-1).args), '{"atLaunch":false}', 'Anything but true is not the launch check');
  run(`settings.network = { ...settings.network, offline: true }`);
  await run('checkUpdates()');
  assert.equal(element('toast').textContent, 'Offline mode is on, so nothing was checked.');
  run(`settings.network = { ...settings.network, offline: false }`);

  // The activity list: escaped, newest first as the core sends it, refusals marked.
  run(`renderActivity({ entries: [
      { at: 0, purpose: 'images', host: 'evil.example', path: '/<img src=x>', outcome: 'refused', detail: 'host not allowed' },
      { at: 0, purpose: 'github', host: 'github.com', path: '/o/r/releases/latest/download/release.json', outcome: '302', detail: 'redirected to release-assets.githubusercontent.com' }
    ], counts: [{ purpose: 'github', sent: 2, refused: 0 }, { purpose: 'images', sent: 0, refused: 1 }] })`);
  const activity = element('net-activity-list').innerHTML;
  assert.match(activity, /\/&lt;img src=x&gt;/);
  assert.doesNotMatch(activity, /<img/);
  assert.match(activity, /net-row refused/);
  assert.ok(activity.indexOf('evil.example') < activity.indexOf('github.com'));
  assert.equal(element('net-activity-sum').textContent, '2 sent, 1 refused: GitHub 2 · Images 0 (1 refused)');
  run(`renderActivity({ entries: [], counts: [] })`);
  assert.equal(element('net-activity-sum').textContent, 'Nothing sent or refused yet.');

  // A download that failed its published-hash check is explained, not shown as a code.
  const mismatch = run(`explainError('HASH_MISMATCH: the downloaded file does not match the SHA-1 its source published for it (expected aa, got bb), so it was deleted and nothing was installed')`);
  assert.match(mismatch, /^Refused: the downloaded file does not match the SHA-1/);
  assert.doesNotMatch(mismatch, /HASH_MISMATCH/);

  run('clearTimeout(toastTimer); clearTimeout(curse.timer)');
  console.log('UI checks passed: counts, eligible updates, failure priority and retry, filters, search, escaping, empty states, modal focus and cancellation, sort direction and remote pagination, saved preferences, backups and folder actions, CurseForge-only rows, reversible removal, the CurseForge API key, self-update being off without a channel, installing from a zip by dialog or drop, and the network settings, launch check and activity list.');
})().catch((error) => { console.error(error); process.exitCode = 1; });
