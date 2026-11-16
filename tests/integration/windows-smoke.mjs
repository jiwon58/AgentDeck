import assert from 'node:assert/strict';
import { execFile, execFileSync, spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { copyFile, mkdir, mkdtemp, readFile, readdir, rename, rm, stat, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import { setTimeout as sleep } from 'node:timers/promises';
import { promisify } from 'node:util';

const application = resolve(process.argv[2] ?? 'target/release/agent-deck.exe');
const fixture = resolve(process.argv[3] ?? 'target/release/mock-agent.exe');
const root = await mkdtemp(join(tmpdir(), 'agent-deck-smoke-'));
let resumedContextSeen = false;
let servedModels = ['mock-local', 'old-model'];
const anthropicToken = 'temporary-fixture-authorization';
const mock = createServer((request, response) => {
  if (request.method === 'POST' && request.url === '/v1/messages') {
    let body = '';
    request.on('data', chunk => { body += chunk; });
    request.once('end', () => {
      if (request.headers.authorization !== 'Bearer ' + anthropicToken) {
        response.writeHead(401).end();
        return;
      }
      const input = JSON.parse(body);
      if (input.model !== 'mock-local') { response.writeHead(404).end(); return; }
      const usage = { input_tokens: 1, output_tokens: 1 };
      const message = { id: 'deck-anthropic', type: 'message', role: 'assistant', model: input.model, content: [], usage };
      if (!input.stream) {
        response.writeHead(200, { 'Content-Type': 'application/json' });
        response.end(JSON.stringify({ ...message, content: [{ type: 'text', text: 'Deck Anthropic fixture reply' }] }));
        return;
      }
      const events = [
        { type: 'message_start', message },
        { type: 'content_block_start', index: 0, content_block: { type: 'text', text: '' } },
        { type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text: 'Deck Anthropic fixture reply' } },
        { type: 'content_block_stop', index: 0 },
        { type: 'message_delta', delta: { stop_reason: 'end_turn', stop_sequence: null }, usage },
        { type: 'message_stop' },
      ];
      response.writeHead(200, { 'Content-Type': 'text/event-stream' });
      response.end(events.map(event => `event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`).join(''));
    });
    return;
  }
  if (request.method === 'POST' && request.url === '/v1/chat/completions') {
    let body = '';
    request.on('data', chunk => { body += chunk; });
    request.once('end', () => {
      const input = JSON.parse(body);
      const messages = input.messages;
      if (input.stream === false) {
        if (!servedModels.includes(input.model)) {
          response.writeHead(404).end('{"error":{"message":"missing model"}}');
        } else {
          response.writeHead(200, { 'Content-Type': 'application/json' })
            .end('{"choices":[{"message":{"content":"OK"}}]}');
        }
        return;
      }
      const ask = messages.at(-1)?.role === 'user' && JSON.stringify(messages.at(-1).content).includes('Deck ask status probe');
      if (messages.at(-1)?.role === 'user' && JSON.stringify(messages.at(-1).content).includes('Deck resumed context probe')) {
        resumedContextSeen = messages.slice(0, -1).some(message =>
          message.role === 'user' && JSON.stringify(message.content).includes('Deck ask status probe'));
      }
      response.writeHead(200, { 'Content-Type': 'text/event-stream' });
      const chunk = (delta, finish_reason) => `data: ${JSON.stringify({
        id: 'deck-smoke', object: 'chat.completion.chunk', model: 'mock-local', created: 0,
        choices: [{ index: 0, delta, finish_reason }],
      })}\n\n`;
      response.write(chunk({ role: 'assistant' }, null));
      setTimeout(() => {
        if (ask) {
          const tool = { index: 0, id: 'call_deck_ask', type: 'function', function: {
            name: 'ask', arguments: JSON.stringify({ questions: [{
              id: 'deck_probe', question: 'Which test option should Deck use?',
              options: [{ label: 'First option' }, { label: 'Second option' }], recommended: 0,
            }] }),
          } };
          response.end(chunk({ content: 'Please choose a test option.' }, null) +
            chunk({ tool_calls: [tool] }, null) + chunk({}, 'tool_calls') + 'data: [DONE]\n\n');
          return;
        }
        response.end(chunk({ content: 'Deck loopback fixture reply' }, null) + chunk({}, 'stop') + 'data: [DONE]\n\n');
      }, messages.at(-1)?.role === 'user' && JSON.stringify(messages.at(-1).content).includes('Deck queue timing probe') ? 8000 : 1500);
    });
    return;
  }
  if (request.url !== '/v1/models') { response.writeHead(404).end(); return; }
  response.writeHead(200, { 'Content-Type': 'application/json' })
    .end(JSON.stringify({ data: servedModels.map(id => ({ id })) }));
});
await new Promise(resolve => mock.listen(0, '127.0.0.1', resolve));
const debugPort = await new Promise(resolve => {
  const listener = createServer();
  listener.listen(0, '127.0.0.1', () => {
    const { port } = listener.address();
    listener.close(() => resolve(port));
  });
});
const pathA = join(root, '런타임 A', 'gjc.exe');
const pathB = join(root, 'runtime B', 'gjc.exe');
const work = join(root, '한글 English workspace');
await mkdir(join(root, '런타임 A'));
await mkdir(join(root, 'runtime B'));
await mkdir(work);
await copyFile(fixture, pathA);
await copyFile(fixture, pathB);
const suffix = process.pid.toString();
const sessionNames = [`Smoke A ${suffix}`, `Smoke B ${suffix}`, `Smoke C ${suffix}`];
const optionalGjc = [['0.10.2', process.env.DECK_GJC_0102], ['0.20.0', process.env.DECK_GJC_0200]];
const actualNames = optionalGjc.map(([version]) => `Smoke GJC ${version} ${suffix}`);
const routerName = `Mock router ${suffix}`;
const fakeToken = `deck-diagnostic-test-token-${suffix}`;
const updatedRouter = createServer();
await new Promise(resolve => updatedRouter.listen(0, '127.0.0.1', resolve));
let app;
let browser;
let presetIds = [];
const presetPrefix = 'Smoke preset ' + suffix;

async function eventually(action, label) {
  const deadline = Date.now() + 25000;
  let lastError;
  while (Date.now() < deadline) {
    try { const result = await action(); if (result) return result; }
    catch (error) { lastError = error; }
    await sleep(100);
  }
  throw new Error(`Timed out waiting for ${label}`, { cause: lastError });
}

async function start() {
  const system = process.env.SystemRoot ?? 'C:\\Windows';
  const environment = Object.fromEntries(Object.entries(process.env).filter(([key]) => key.toLowerCase() !== 'path'));
  app = spawn(application, [], {
    env: { ...environment, PATH: `${system}\\System32;${system}`,
      AGENT_DECK_TEST_DATA_DIR: join(root, 'deck-data'),
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${debugPort} --no-proxy-server --host-resolver-rules="MAP * ~NOTFOUND, EXCLUDE localhost, EXCLUDE tauri.localhost, EXCLUDE ipc.localhost"`,
      WEBVIEW2_USER_DATA_FOLDER: join(root, 'webview') },
  });
  app.on('error', error => console.error(error));
  const endpoint = await eventually(async () => {
    const response = await fetch(`http://127.0.0.1:${debugPort}/json/list`);
    return (await response.json()).find(page => page.type === 'page' && page.url.includes('tauri.localhost'))?.webSocketDebuggerUrl;
  }, 'Agent Deck WebView');
  browser = new WebSocket(endpoint);
  await new Promise((resolve, reject) => {
    browser.addEventListener('open', resolve, { once: true });
    browser.addEventListener('error', reject, { once: true });
  });
  const pending = new Map();
  let id = 0;
  browser.addEventListener('message', ({ data }) => {
    const message = JSON.parse(data);
    if (message.id && pending.has(message.id)) {
      const { resolve, reject } = pending.get(message.id);
      pending.delete(message.id);
      if (message.error) reject(new Error(message.error.message));
      else resolve(message.result);
    }
  });
  browser.addEventListener('close', () => {
    for (const { reject } of pending.values()) reject(new Error('WebView closed'));
    pending.clear();
  });
  return async (method, params = {}) => {
    const key = ++id;
    return new Promise((resolve, reject) => {
      pending.set(key, { resolve, reject });
      browser.send(JSON.stringify({ id: key, method, params }));
    });
  };
}

let send;
async function evaluate(fn, ...args) {
  const reply = await send('Runtime.evaluate', {
    expression: `(${fn})(...${JSON.stringify(args)})`, awaitPromise: true, returnByValue: true,
  });
  if (reply.exceptionDetails) throw new Error(reply.exceptionDetails.exception?.description ?? reply.exceptionDetails.text);
  return reply.result.value;
}
const ready = () => eventually(() => evaluate(() => !document.querySelector('.topbar .accent')?.disabled && !!document.querySelector('.topbar')), 'desktop UI');
const waitText = (selector, phrase) => eventually(() => evaluate((selector, phrase) =>
  Array.from(document.querySelectorAll(selector)).some(element => element.textContent.includes(phrase)), selector, phrase), phrase);
async function click(text, index = 0) {
  await eventually(() => evaluate((text, index) => {
    const scope = document.querySelector('dialog[open]') ?? document;
    const button = Array.from(scope.querySelectorAll('button')).filter(button =>
      (button.textContent.trim() === text || button.getAttribute('aria-label') === text) && button.getClientRects().length)[index];
    if (!button || button.matches(':disabled')) return false;
    button.click();
    return true;
  }, text, index), `button ${text}`);
}
async function field(labelText, value) {
  await eventually(() => evaluate((labelText, value) => {
    const scope = document.querySelector('dialog[open]') ?? document;
    const label = Array.from(scope.querySelectorAll('label')).find(label =>
      label.getClientRects().length && label.textContent.trim().startsWith(labelText));
    const input = label?.querySelector('input,select,textarea') ??
      Array.from(scope.querySelectorAll('input,select,textarea')).find(element =>
        element.getClientRects().length && element.getAttribute('aria-label') === labelText);
    if (!input || input.disabled) return false;
    if (input.type === 'checkbox') {
      if (input.checked !== value) input.click();
    } else {
      Object.getOwnPropertyDescriptor(Object.getPrototypeOf(input), 'value').set.call(input, String(value));
      input.dispatchEvent(new Event('input', { bubbles: true }));
      input.dispatchEvent(new Event('change', { bubbles: true }));
    }
    return true;
  }, labelText, value), `field ${labelText}`);
}
const invoke = (name, data) => evaluate(async (name, data) => {
  try { return await window.__TAURI_INTERNALS__.invoke(name, data); }
  catch (error) { throw new Error(String(error)); }
}, name, data);
const overview = () => invoke('overview', {});

const launchForPreset = (preset, working, values = {}) => ({
  id: crypto.randomUUID(), name: preset.name, cwd: working, cols: 160, rows: 24,
  gjc: { preset_id: preset.id, preview_fingerprint: '' }, ...values,
});

async function startPreset(launch, preview) {
  launch.gjc.preview_fingerprint = preview.fingerprint;
  return invoke('start_session', { launch });
}

async function terminalOutput(id, pattern) {
  let output = '';
  let answered = 0;
  const ended = [];
  try {
    await eventually(async () => {
      for (const event of await invoke('terminal_events', { id })) {
        if (event.event_type === 'terminal_output') output += Buffer.from(event.payload).toString();
        if (event.event_type === 'terminal_exited') ended.push(event.payload);
      }
      const queries = output.split('\x1b[6n').length - 1;
      while (answered < queries) {
        // ConPTY asks for the cursor position before it delivers its first frame.
        await invoke('write_terminal', { id, data: '\x1b[1;1R' });
        answered++;
      }
      return pattern.test(output.replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, ''));
    }, String(pattern));
  } catch (error) {
    throw new Error(`Expected ${pattern}; last output: ${JSON.stringify(output.slice(-1600))}; exit: ${JSON.stringify(ended)}`, { cause: error });
  }
  return output;
}

function presetDocument(runtime, profile, name, agentPath) {
  return {
    id: '', name, enabled: true, auto_config_enabled: true, runtime_id: runtime.id,
    router_id: profile.id, protocol: 'openai', provider_id: 'deck-preset', model: 'mock-local',
    context_tokens: 16384, max_output_tokens: 1024, directory_mode: agentPath ? 'custom' : 'managed',
    agent_path: agentPath || '', history_mode: 'existing_workspace',
    stream_timeout: { enabled: true, value: '900000' }, model_preset_registry: { enabled: true, value: '1' },
    variables: [{ key: 'DECK_FIXTURE_MODE', value: 'validated', enabled: true }],
  };
}

async function presetSmoke() {
  const data = await overview();
  const runtime = data.runtimes.find(item => item.executable.path === pathB && item.agent === 'gajae');
  assert(runtime);
  const profile = await invoke('save_router', { profile: {
    id: '', name: presetPrefix + ' router', scheme: 'http', host: '127.0.0.1', port: mock.address().port,
    openai_path: '/v1', anthropic_path: null, model: '', authentication: 'none', enabled: true,
    allow_remote: false, created_at: 0, updated_at: 0,
  } });
  const draft = presetDocument(runtime, profile, presetPrefix + ' mock');
  await stop();
  send = await start();
  await ready();
  await click('Settings');
  await click('GJC Presets');
  await click('New preset');
  await field('Name', draft.name);
  await field('Runtime', runtime.id);
  await field('Router', profile.id);
  await field('Model ID', draft.model);
  await field('Provider ID', draft.provider_id);
  await field('Session history', 'existing_workspace');
  await click('Save preset');
  const preset = await eventually(async () => {
    const failure = await evaluate(() => document.querySelector('dialog [role="alert"]')?.textContent);
    if (failure) throw new Error(failure);
    return (await overview()).gjc_presets.find(item => item.name === draft.name);
  }, 'preset CRUD via Settings');
  presetIds.push(preset.id);
  await field('Working directory', work);
  await click('Validate and preview');
  await waitText('dialog[open]', 'Launch preview · validated');
  await click('Duplicate');
  await click('Save preset');
  const copy = await eventually(async () => (await overview()).gjc_presets.find(item => item.name === draft.name + ' (copy)'), 'duplicated preset');
  presetIds.push(copy.id);
  assert.notEqual(copy.id, preset.id);
  const originalPreview = await invoke('preview_gjc_preset', { launch: launchForPreset(preset, work) });
  const copyPreview = await invoke('preview_gjc_preset', { launch: launchForPreset(copy, work) });
  assert.notEqual(originalPreview.plan.models_target, copyPreview.plan.models_target);
  await invoke('delete_gjc_preset', { id: copy.id, revision: copy.revision });
  await click('Close dialog');
  const secondWork = join(root, 'second workspace');
  await mkdir(secondWork);
  const launch = launchForPreset(preset, work, { env: { DECK_FIXTURE_MODE: 'validated' } });
  const second = launchForPreset(preset, secondWork);
  const preview = await invoke('preview_gjc_preset', { launch });
  const other = await invoke('preview_gjc_preset', { launch: second });
  assert.deepEqual(preview.plan.argv, other.plan.argv);
  assert.equal(preview.plan.models_target, other.plan.models_target);
  assert.notEqual(preview.plan.history.session_directory, other.plan.history.session_directory);
  await assert.rejects(stat(preview.plan.models_target), { code: 'ENOENT' });
  const firstSession = await startPreset(launch, preview);
  assert.equal(firstSession.gjc.models_fingerprint, preview.plan.history.models_fingerprint);
  const created = await stat(preview.plan.models_target);
  await terminalOutput(firstSession.id, /Mock Agent ready/);
  await invoke('write_terminal', { id: firstSession.id, data: 'inspect launch\r' });
  const output = await terminalOutput(firstSession.id, /fixture mode=validated/);
  assert(output.includes('fixture argv=' + JSON.stringify(preview.plan.argv).replaceAll(',', ', ')));
  assert(output.includes('fixture timeout=' + preview.plan.env.GJC_OPENAI_STREAM_IDLE_TIMEOUT_MS));
  const secondSession = await startPreset(second, other);
  assert.equal((await stat(preview.plan.models_target)).mtimeMs, created.mtimeMs);

  const changed = await invoke('save_gjc_preset', { preset: { ...preset, model: 'other-model' } });
  await assert.rejects(startPreset(launchForPreset(changed, work), preview), /configuration changed/);
  const changedLaunch = launchForPreset(changed, work);
  const changedPreview = await invoke('preview_gjc_preset', { launch: changedLaunch });
  await assert.rejects(startPreset(changedLaunch, changedPreview), /live session/);
  await invoke('close_terminal', { id: firstSession.id });
  await invoke('close_terminal', { id: secondSession.id });
  const third = await startPreset(changedLaunch, changedPreview);
  await invoke('close_terminal', { id: third.id });

  await stop();
  send = await start();
  await ready();
  await click('＋ New Session');
  if (!await evaluate(() => [...document.querySelectorAll('dialog label')].some(label => label.textContent.trim().startsWith('GJC preset')))) {
    await click('GJC preset');
  }
  await field('GJC preset', changed.id);
  await field('Working directory', work);
  await click('Preview launch');
  await waitText('dialog[open]', 'Launch preview · validated');
  await click('Start session');
  await waitText('.terminal-pane:not([hidden]) .xterm-rows', 'Mock Agent ready');

  for (const [version, binary] of optionalGjc) {
    if (!binary) continue;
    const current = await overview();
    const runtime = current.runtimes.find(item => item.name === `Smoke runtime ${suffix}-gjc-${version}`);
    const sandbox = join(root, `preset-home-${version}`);
    await mkdir(sandbox);
    const env = { ...process.env, GJC_CODING_AGENT_DIR: sandbox, GJC_MODEL_PRESET_REGISTRY_DISABLED: '1', GJC_UI_LANGUAGE: 'en' };
    execFileSync(binary, ['config', 'set', 'startup.checkUpdate', 'false'], { cwd: work, env });
    execFileSync(binary, ['config', 'set', 'tools.discoveryMode', 'off'], { cwd: work, env });
    await mkdir(join(sandbox, 'onboarding'), { recursive: true });
    await writeFile(join(sandbox, 'onboarding', 'frictionless.json'), JSON.stringify({ version: 1, decision: 'skipped' }));
    const preset = await invoke('save_gjc_preset', { preset: presetDocument(runtime, profile, presetPrefix + ' ' + version, sandbox) });
    presetIds.push(preset.id);
    const launch = launchForPreset(preset, work);
    const preview = await invoke('preview_gjc_preset', { launch });
    assert.match(preview.plan.history.session_directory, version === '0.10.2' ? /[\\/]sessions[\\/]-/ : /[\\/]sessions[\\/]v2-[a-z2-7]{52}$/);
    const session = await startPreset(launch, preview);
    await invoke('close_terminal', { id: session.id });
    const exec = promisify(execFile);
    const listed = await exec(binary, ['--list-models', 'mock-local'], { cwd: work, env, timeout: 15000 });
    assert.match(listed.stdout, /mock-local/);
    const native = await exec(binary, ['--model', 'deck-preset/mock-local', '--print', 'Deck history parity probe'], { cwd: work, env, timeout: 25000 });
    assert.match(native.stdout, /Deck loopback fixture reply/);
    const files = await readdir(preview.plan.history.session_directory);
    assert(files.some(name => name.endsWith('.jsonl')));
    if (version === '0.20.0') assert(files.includes('.gjc-managed-session-scope.v2.json'));
    const resume = launchForPreset(preset, work, { resume_from: session.id });
    const review = await invoke('preview_gjc_preset', { launch: resume });
    assert.equal(review.plan.argv.at(-1), '--resume');
    assert.equal(review.plan.history.session_directory, preview.plan.history.session_directory);
    if (version === '0.10.2') assert.equal(review.plan.argv.at(-3), '--session-dir');
    else assert(!review.plan.argv.includes('--session-dir'));
    const resumed = await startPreset(resume, review);
    await terminalOutput(resumed.id, /Resume Session/);
    await invoke('close_terminal', { id: resumed.id });
    console.log(`GJC ${version} preset smoke passed: --list-models, loopback inference, native scope parity, and Resume picker.`);
    if (version === '0.20.0') {
      const anthropic = await invoke('save_router', { profile: {
        ...profile, id: '', name: presetPrefix + ' anthropic', authentication: 'bearer', openai_path: null, anthropic_path: '/v1',
      } });
      const result = await invoke('test_router_model', { profile: anthropic, token: anthropicToken, protocol: 'anthropic', model: 'mock-local' });
      assert.equal(result.responded, true);
      const updated = await invoke('save_gjc_preset', { preset: { ...preset, router_id: anthropic.id, protocol: 'anthropic' } });
      const launch = launchForPreset(updated, work, {
        token: anthropicToken, extra: ['--print', 'Deck Anthropic integration probe'],
      });
      const review = await invoke('preview_gjc_preset', { launch });
      assert.equal(review.plan.endpoint, `http://127.0.0.1:${mock.address().port}`);
      assert.equal(review.plan.requires_token, true);
      assert(!JSON.stringify(review).includes(anthropicToken));
      const running = await startPreset(launch, review);
      await terminalOutput(running.id, /Deck Anthropic fixture reply/);
      await invoke('close_terminal', { id: running.id });
      assert(!(await readFile(review.plan.models_target, 'utf8')).includes(anthropicToken));
      assert(!JSON.stringify(await overview()).includes(anthropicToken));
      await invoke('delete_router', { id: anthropic.id });
      console.log('Real GJC Anthropic Messages smoke passed with a temporary bearer credential.');
    }
  }
  console.log('GJC preset smoke passed: Settings, New Session, validated argv/env, concurrent reuse, stale preview, and live configuration conflict.');
  for (const agent of ['claude', 'hermes', 'generic']) {
    const runtime = await registerFixture(pathB, '0.20.0', `Smoke runtime ${suffix}-regression-${agent}`, false, agent);
    const launch = { id: crypto.randomUUID(), name: presetPrefix + ' ' + agent, cwd: work, runtime_id: runtime.id, rows: 24, cols: 120 };
    const session = await invoke('start_session', { launch });
    assert.equal(session.agent, agent);
    assert.equal(session.gjc, null);
    await terminalOutput(session.id, /Mock Agent ready/);
    await invoke('write_terminal', { id: session.id, data: 'inspect launch\r' });
    const output = await terminalOutput(session.id, /fixture mode=/);
    assert(!output.includes('fixture host=terax'));
    assert(!output.includes('fixture timeout=900000'));
    assert(output.includes('fixture argv=[]'));
    await invoke('close_terminal', { id: session.id });
  }
  console.log('Claude, Hermes and Generic runtime regression passed with a fixture executable.');
}

async function registerFixture(path, version, name, real = false, agent = 'gajae') {
  const candidate = await invoke('inspect_executable', { path });
  return invoke('register_runtime', { request: {
    agent, name, candidate, arguments: [],
    probe_args: real ? [] : ['--mock-version=' + version],
    root: '', cwd_override: null, dependencies: '', router_environment: {}, approved: true, probe: true,
  } });
}
async function selectTab(label) {
  await eventually(() => evaluate(label => {
    const button = Array.from(document.querySelectorAll('.tab button:first-child')).find(button => button.textContent.includes(label));
    if (!button) return false;
    button.click();
    return true;
  }, label), label);
}
async function terminalInput(text) {
  await evaluate(() => document.querySelector('.terminal-pane:not([hidden]) textarea').focus());
  await send('Input.insertText', { text });
  await send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13, text: '\r' });
  await send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13 });
}
async function keypress(key, code, windowsVirtualKeyCode, modifiers = 0) {
  const event = { key, code, windowsVirtualKeyCode, modifiers };
  await send('Input.dispatchKeyEvent', { type: 'rawKeyDown', ...event });
  await send('Input.dispatchKeyEvent', { type: 'keyUp', ...event });
}

async function checkScrollbar() {
  const selector = '.terminal-pane:not([hidden]) .terminal-scrollbar';
  const position = () => evaluate(selector => document.querySelector(selector).valueAsNumber, selector);
  await terminalInput('scrollback 160');
  await waitText('.terminal-pane:not([hidden]) .xterm-rows', 'stream 159');
  const bottom = await eventually(() => evaluate(selector => {
    const range = document.querySelector(selector);
    return +range.max > 100 && range.valueAsNumber === +range.max && +range.max;
  }, selector), 'terminal scrolled to bottom');
  const box = await evaluate(selector => {
    const viewport = document.querySelector(selector);
    const bounds = viewport.getBoundingClientRect();
    return { x: Math.floor(bounds.right - 6), top: Math.ceil(bounds.top), height: viewport.clientHeight };
  }, selector);
  const pointer = (type, y, buttons) => send('Input.dispatchMouseEvent', {
    type, x: box.x, y, button: 'left', buttons, clickCount: type === 'mouseMoved' ? 0 : 1,
  });
  await pointer('mousePressed', box.top + 8, 1);
  await pointer('mouseReleased', box.top + 8, 0);
  await eventually(async () => await position() === 0, 'clicking the scrollbar track moves the terminal to the top');
  await waitText('.terminal-pane:not([hidden]) .xterm-rows', 'stream 0');
  await pointer('mousePressed', box.top + 8, 1);
  await pointer('mouseMoved', box.top + Math.floor(box.height * 0.75), 1);
  await pointer('mouseMoved', box.top + box.height - 2, 1);
  await pointer('mouseReleased', box.top + box.height - 2, 0);
  await eventually(async () => await position() > bottom / 2, 'dragging the scrollbar thumb moves the terminal');
  await waitText('.terminal-pane:not([hidden]) .xterm-rows', 'stream 159');
  console.log('Terminal scrollbar passed: clicking the track and dragging the thumb move the viewport.');
}

async function conversationFiles(root) {
  return (await readdir(root, { recursive: true })).filter(path => path.endsWith('.jsonl')).sort();
}

async function checkSessionTools() {
  const rows = '.terminal-pane:not([hidden]) .xterm-rows';
  await click('Find');
  await field('Find in terminal output', 'stream 22 ');
  await waitText('.terminal-find output', '1 / 1');
  await click('Next match');
  await waitText(rows, 'stream 22');
  await click('Bookmark');
  await click('Close terminal search');
  await click('Latest');
  await waitText(rows, 'stream 159');
  await evaluate(() => document.querySelector('.terminal-pane:not([hidden]) .terminal-bookmarks summary').click());
  await eventually(() => evaluate(() => {
    const jump = document.querySelector('.terminal-pane:not([hidden]) .bookmark-list button');
    if (!jump) return false;
    jump.click();
    return true;
  }), 'bookmark button');
  await waitText(rows, 'stream 22');
  await click('Latest');
  await click('Read-only');
  await eventually(() => evaluate(() => document.querySelector('.terminal-pane:not([hidden]) button[aria-pressed="true"]')?.textContent === 'Read-only'), 'read-only enabled');
  await evaluate(() => {
    const clipboardData = new DataTransfer();
    clipboardData.setData('text/plain', 'blocked clipboard');
    document.querySelector('.terminal-pane:not([hidden]) textarea').dispatchEvent(new ClipboardEvent('paste', { clipboardData, bubbles: true, cancelable: true }));
  });
  await terminalInput('read-only must block this draft');
  await sleep(180);
  assert(!await evaluate(selector => document.querySelector(selector).textContent.includes('read-only must block this draft'), rows));
  await click('Read-only');
  await eventually(() => evaluate(() => document.querySelector('.terminal-pane:not([hidden]) button[aria-pressed="false"]')?.textContent === 'Read-only'), 'read-only disabled');
  await terminalInput('read-only released');
  try { await waitText(rows, 'echo: read-only released'); }
  catch (error) {
    console.error(await evaluate(selector => ({
      viewport: document.querySelector('.terminal-pane:not([hidden]) .terminal-scrollbar').value,
      focus: document.activeElement?.className,
      lastRows: document.querySelector(selector)?.innerText.split('\n').slice(-8),
    }), rows));
    throw error;
  }

  await evaluate(() => { window.deckOriginalTerminal = document.querySelector('.terminal-pane:not([hidden]) .xterm'); });
  await click('Side by side');
  await field('Search sessions', sessionNames[0]);
  await evaluate(() => document.querySelector('dialog[open] input[aria-label="Search sessions"]').focus());
  await keypress('Enter', 'Enter', 13);
  await eventually(() => evaluate(() => document.querySelectorAll('.terminal-pane:not([hidden])').length === 2 &&
    document.querySelector('.pane-left .xterm').isSameNode(window.deckOriginalTerminal)), 'two existing terminal panes');
  for (const [side, name] of [['left', sessionNames[2]], ['right', sessionNames[0]]]) {
    await evaluate(side => document.querySelector('.pane-' + side + ' textarea').focus(), side);
    await waitText('.tab.selected', name);
    await send('Input.insertText', { text: 'split ' + side + ' input' });
    await keypress('Enter', 'Enter', 13);
    await waitText('.pane-' + side + ' .xterm-rows', 'echo: split ' + side + ' input');
  }
  assert(!await evaluate(() => document.querySelector('.pane-left .xterm-rows').textContent.includes('split right input')));
  await click('Close split');
  assert.equal(await evaluate(() => document.querySelectorAll('.terminal-pane:not([hidden])').length), 1);

  const first = (await overview()).sessions.find(session => session.name === sessionNames[0]);
  await invoke('write_terminal', { id: first.id, data: 'host-status attention\r' });
  await click('Next reply (2)');
  await waitText('.tab.selected', sessionNames[1]);
  await evaluate(() => document.querySelector('.help-button').focus());
  await keypress('k', 'KeyK', 75, 2);
  await waitText('dialog[open] h2', 'Switch session');
  await field('Search sessions', sessionNames[2]);
  await evaluate(() => document.querySelector('dialog[open] input[aria-label="Search sessions"]').focus());
  await keypress('Enter', 'Enter', 13);
  await waitText('.tab.selected', sessionNames[2]);
  await click('Pin ' + sessionNames[1]);
  await field('Group by project', true);
  await waitText('.session-group-name', 'Pinned');
  await waitText('.session-group .session-group-name', work);
  console.log('Session tools passed: output search, bookmark, latest, read-only, live split, reply navigation, switcher, grouping and pins.');
}

async function checkInstallSettings() {
  await click('Settings');
  await click('Install Agents');
  await waitText('dialog[open]', 'Destination directory');
  await field('Agent', 'hermes');
  await waitText('dialog[open]', 'Download official installer');
  await click('Close dialog');
  assert.match(await invoke('download_agent', { agent: 'generic', directory: root }).catch(error => error.message), /No publisher download/);
  if (process.env.DECK_PUBLISHER_DOWNLOADS !== '1') return;
  const directory = join(root, 'publisher downloads');
  await mkdir(directory);
  const claude = await invoke('download_agent', { agent: 'claude', directory });
  assert.match(claude.version, /^\d+\.\d+\.\d+$/);
  assert.equal((await invoke('inspect_executable', { path: claude.path })).sha256, claude.sha256);
  assert.match(execFileSync(claude.path, ['--version'], { cwd: directory, timeout: 30000 }).toString(), /Claude Code/);
  const hermes = await invoke('download_agent', { agent: 'hermes', directory });
  const home = join(directory, 'hermes-home');
  const checkout = join(directory, 'hermes-agent');
  const powershell = join(process.env.SystemRoot, 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe');
  const paths = execFileSync(powershell, ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'RemoteSigned', '-File', hermes.path,
    '-HermesHome', home, '-InstallDir', checkout, '-NonInteractive', '-ShowResolvedPaths'], { cwd: directory, timeout: 30000 }).toString();
  assert.equal(JSON.parse(paths).hermes_home.toLowerCase(), home.toLowerCase());
  await writeFile(hermes.path, 'modified fixture');
  await assert.rejects(invoke('install_hermes', { sha256: hermes.sha256 }), /installer changed/i);
  console.log('Publisher downloads passed: Claude native binary and checksum, Hermes official bootstrap and destination paths, tampered script rejected.');
}
async function stop() {
  const closed = new Promise(resolve => app.once('exit', resolve));
  try { await invoke('quit', {}); } catch { /* The WebView can close before the reply. */ }
  await Promise.race([closed, sleep(8000).then(() => { if (app.exitCode === null) throw new Error('Application did not close'); })]);
  browser.close();
}

try {
  send = await start();
  await ready();
  assert.equal((await overview()).data_directory.toLowerCase(), join(root, 'deck-data').toLowerCase(),
    'Use a test-fixture executable so the smoke suite can isolate its database and logs');
  if (process.env.DECK_INSTALL_ONLY === '1') {
    await checkInstallSettings();
  } else if (process.env.DECK_GJC_PRESET_ONLY === '1') {
    await registerFixture(pathB, '0.20.0', `Smoke runtime ${suffix}-fixture`);
    for (const [version, binary] of optionalGjc) {
      if (binary) await registerFixture(binary, version, `Smoke runtime ${suffix}-gjc-${version}`, true);
    }
  } else {
  const initialVersion = (await overview()).app_version;
  assert.match(initialVersion, /^\d+\.\d+\.\d+/);
  await waitText('.statusbar .app-version', `Agent Deck v${initialVersion}`);
  await click('Keyboard shortcuts');
  await waitText('dialog[open] h2', 'Keyboard shortcuts');
  await waitText('dialog[open]', 'Focus a Deck control');
  await keypress('Escape', 'Escape', 27);
  await eventually(() => evaluate(() => !document.querySelector('dialog[open]')), 'shortcut guide close');
  await evaluate(() => document.querySelector('.help-button').focus());
  await keypress('t', 'KeyT', 84, 2);
  await waitText('dialog[open] h2', 'New Session');
  await click('Close dialog');
  await click('Settings');
  await field('Installation root', root);
  await click('Scan');
  await waitText('.settings-body', pathA);
  await waitText('.settings-body', pathB);
  for (const [index, path, version] of [[0, pathA, '0.10.2'], [1, pathB, '0.20.0']]) {
    const row = await evaluate(path => Array.from(document.querySelectorAll('.data-row')).find(row => row.textContent.includes(path) && row.textContent.includes('Review'))?.querySelector('button')?.textContent, path);
    assert.equal(row, 'Review');
    await eventually(() => evaluate(path => {
      const row = Array.from(document.querySelectorAll('.data-row')).find(row => row.textContent.includes(path));
      const button = row?.querySelector('button');
      if (!button || button.matches(':disabled')) return false;
      button.click();
      return true;
    }, path), 'review candidate');
    await field('Runtime name', `Smoke runtime ${suffix}-${index}`);
    await field('Version probe arguments', `--mock-version=${version}`);
    await field('Router environment bindings', 'DECK_MOCK_ROUTER=endpoint');
    await field('I trust this executable', true);
    await click('Register runtime');
    await waitText('.data-row strong', `Smoke runtime ${suffix}-${index} · ${version}`);
  }
  const runtimes = (await overview()).runtimes.filter(runtime => runtime.name.startsWith(`Smoke runtime ${suffix}-`));
  assert.equal(runtimes.length, 2);
  await click('Local LLM Router');
  await field('Profile name', routerName);
  await field('Port', mock.address().port);
  await field('Authentication', 'bearer');
  await field('Temporary connection test token', fakeToken);
  await click('Test Connection');
  await waitText('.settings-body', 'Model list available');
  await click('Refresh models');
  await waitText('.settings-body', '2 listed');
  await field('Discovered models', 'old-model');
  await click('Test model');
  await waitText('.settings-body', 'Model responded · HTTP 200');
  await click('Use as default');
  servedModels = ['mock-local', 'new-model'];
  await click('Refresh models');
  await waitText('.settings-body', 'Default model is absent');
  await waitText('.settings-body select option', 'new-model');
  assert.equal(await evaluate(() => [...document.querySelectorAll('[aria-label="Router models"] option')]
    .some(option => option.value === 'old-model')), false);
  await field('Model ID to test', 'old-model');
  await click('Test model');
  await waitText('.settings-body', 'Model or API route not found · HTTP 404');
  await field('Default model', '');
  await field('Authentication', 'none');
  await click('Save profile');
  const router = await eventually(async () => (await overview()).routers.find(profile => profile.name === routerName), 'saved router');
  await click('Close dialog');
  for (const [index, name] of sessionNames.entries()) {
    if (index === 2) {
      await click('Settings');
      await click('Local LLM Router');
      await field('Router profile', router.id);
      await field('Port', updatedRouter.address().port);
      await click('Save profile');
      await eventually(async () => (await overview()).routers.find(profile =>
        profile.id === router.id && profile.port === updatedRouter.address().port), 'updated router');
      await click('Close dialog');
    }
    await click('＋ New Session');
    await field('Session name', name);
    await field('Runtime version', runtimes[index % 2].id);
    await field('Working directory', work);
    await field('Router profile', router.id);
    await field('I configured this Agent', true);
    await click('Start session');
    await waitText('aside', `${index + 1} active`);
    await waitText('.terminal-pane:not([hidden]) .xterm-rows', 'Mock Agent ready');
    assert.equal(await evaluate(() => [...document.querySelectorAll('.terminal-pane:not([hidden]) .xterm-fg-2')]
      .some(element => element.textContent.includes('Mock Agent'))), true);
    await waitText('footer', `127.0.0.1:${index === 2 ? updatedRouter.address().port : router.port}`);
  }
  for (const name of sessionNames) {
    await selectTab(name);
    await terminalInput(`input ${name}`);
    await waitText('.terminal-pane:not([hidden]) .xterm-rows', `echo: input ${name}`);
  }
  const waitStatus = async (name, label, color) => {
    await eventually(() => evaluate((name, label, color) => {
      const row = Array.from(document.querySelectorAll('.session-row')).find(row => row.querySelector('strong')?.textContent === name);
      return row?.querySelector('.session-status')?.textContent === label && !!row.querySelector(`.dot.${color}`);
    }, name, label, color), label);
  };
  await waitStatus(sessionNames[0], 'Terminal running · no Agent status reported', 'saved');
  assert.equal(await evaluate(() => document.querySelectorAll('#session-list select').length), 0);
  await selectTab(sessionNames[2]);
  await checkScrollbar();
  for (const [event, label] of [
    ['working', 'Work in progress'], ['attention', 'Your response needed'], ['finished', 'Agent run finished · check result'],
  ]) {
    await terminalInput(`host-status ${event}`);
    await eventually(() => evaluate(label => document.querySelector('.tab.selected i')?.getAttribute('aria-label') === label,
      label), `GJC host marker ${event}`);
  }
  const currentSessions = (await overview()).sessions;
  await invoke('write_terminal', { id: currentSessions.find(item => item.name === sessionNames[0]).id, data: 'host-status working\r' });
  await invoke('write_terminal', { id: currentSessions.find(item => item.name === sessionNames[1]).id, data: 'host-status attention\r' });
  await waitStatus(sessionNames[0], 'Work in progress', 'in_progress');
  await waitStatus(sessionNames[1], 'Your response needed', 'waiting_for_reply');
  await waitStatus(sessionNames[2], 'Agent run finished · check result', 'completed');
  await checkSessionTools();
  const narrow = await evaluate(() => document.querySelector('main').clientWidth);
  assert.equal(await evaluate(() => document.querySelector('.aside-actions span').nextElementSibling.textContent), '<');
  await click('Collapse sessions');
  await eventually(() => evaluate(narrow => document.querySelector('#session-list').hidden &&
    document.querySelector('.session-toggle').textContent === '>' &&
    document.querySelector('.session-toggle').getAttribute('aria-expanded') === 'false' &&
    document.querySelector('main').clientWidth > narrow, narrow), 'collapsed session list widens terminal');
  await click('Expand sessions');
  await waitText('#session-list:not([hidden]) .session-row', 'Your response needed');
  await waitText('.statusbar .app-version', `Agent Deck v${initialVersion}`);
  await evaluate(() => {
    document.querySelector('.terminal-pane:not([hidden]) textarea').focus();
    window.deckTestKeys = [];
    document.addEventListener('keydown', event => {
      if (event.target.closest('.terminal-pane') && event.ctrlKey) window.deckTestKeys.push(event.key);
    }, true);
  });
  await keypress('t', 'KeyT', 84, 2);
  await keypress('w', 'KeyW', 87, 2);
  await keypress('Tab', 'Tab', 9, 2);
  await keypress('Tab', 'Tab', 9, 10);
  assert.deepEqual(await evaluate(() => window.deckTestKeys), ['t', 'w', 'Tab', 'Tab']);
  assert.equal(await evaluate(() => !!document.querySelector('dialog[open]')), false);
  await waitText('.tab.selected', sessionNames[2]);
  await evaluate(() => document.querySelector('.help-button').focus());
  await keypress('Tab', 'Tab', 9, 2);
  await waitText('.tab.selected', sessionNames[0]);
  await keypress('Tab', 'Tab', 9, 10);
  await waitText('.tab.selected', sessionNames[2]);
  console.log('Shortcut smoke passed: help, keyboard dismissal, Deck tab cycling, and terminal focus isolation.');
  await selectTab(sessionNames[0]);
  await waitText('.terminal-pane:not([hidden]) .xterm-rows', `echo: input ${sessionNames[0]}`);
  await terminalInput('exit 7');
  await waitText('footer', 'code 7');
  await evaluate(() => document.querySelector('.help-button').focus());
  await keypress('w', 'KeyW', 87, 2);
  await eventually(() => evaluate(label => !Array.from(document.querySelectorAll('.tab')).some(tab =>
    tab.textContent.includes(label)), sessionNames[0]), 'Deck shortcut closes the exited tab');
  await selectTab(sessionNames[1]);
  await waitText('footer', `127.0.0.1:${router.port}`);
  await stop();

  send = await start();
  await ready();
  await waitText('aside', sessionNames[2]);
  await waitText('aside', '0 active');
  const saved = (await overview()).sessions.filter(session => sessionNames.includes(session.name));
  assert.equal(saved.length, 3);
  const { session_layout: layout } = await overview();
  assert.equal(layout.grouped, true);
  assert(layout.pinned.includes(saved.find(session => session.name === sessionNames[1]).id));
  await waitStatus(sessionNames[1], 'Saved session · no process running', 'saved');
  const missing = saved.find(session => session.executable.path === pathA);
  assert(missing);
  await rename(pathA, `${pathA}.moved`);
  await eventually(() => evaluate(name => {
    const button = Array.from(document.querySelectorAll('aside button')).find(button => button.textContent.includes(name));
    if (!button) return false;
    button.click();
    return true;
  }, missing.name), 'saved session');
  await click('Restart as new process');
  await waitText('[role="alert"]', 'Executable unavailable');
  console.log('Windows WebView smoke passed: trust, versions, router probe, three interactive tabs, ANSI, exit, saved sessions, missing executable.');

  await click('Settings');
  await click('Application & Offline Settings');
  const { log_file: logFile } = await overview();
  assert(logFile);
  await waitText('.settings-body', logFile);
  await click('Close dialog');
  await eventually(async () => {
    const text = await readFile(logFile, 'utf8');
    const records = text.trim().split('\n').map(line => JSON.parse(line));
    const sessionId = saved.find(item => item.name === sessionNames[0]).id;
    const exit = records.find(item => item.event === 'terminal.exited' && item.entity_id === sessionId && item.code === 7);
    if (!exit) return false;
    assert(records.some(item => item.run_id === exit.run_id && item.event === 'router.probe_result' && item.code === 200));
    assert(records.some(item => item.event === 'session.restore_validation_failed' && item.run_id !== exit.run_id));
    for (const privateValue of [fakeToken, sessionNames[0], JSON.stringify(pathA).slice(1, -1), 'echo: input']) {
      assert(!text.includes(privateValue), 'diagnostic log contained private smoke-test data');
    }
    return true;
  }, 'redacted file diagnostics');
  console.log('Diagnostic log smoke passed: path shown in Settings, exit code and router status persisted, private input omitted.');

  for (const [index, [version, binary]] of optionalGjc.entries()) {
    if (!binary) continue;
    const sandbox = join(root, `gjc-home-${version}`);
    await mkdir(sandbox);
    const env = { USERPROFILE: sandbox, HOME: sandbox, APPDATA: sandbox, GJC_CODING_AGENT_DIR: sandbox,
      GJC_CONFIG_DIR: '.gjc-smoke', GJC_MODEL_PRESET_REGISTRY_DISABLED: '1', GJC_UI_LANGUAGE: 'en' };
    execFileSync(binary, ['config', 'set', 'startup.checkUpdate', 'false'], { cwd: work, env: { ...process.env, ...env } });
    const checkRealMarkers = process.env.DECK_GJC_CHECK_STATUS === '1';
    if (checkRealMarkers) {
      execFileSync(binary, ['config', 'set', 'tools.discoveryMode', 'off'], { cwd: work, env: { ...process.env, ...env } });
      await mkdir(join(sandbox, 'onboarding'));
      await writeFile(join(sandbox, 'onboarding', 'frictionless.json'), JSON.stringify({ version: 1, decision: 'skipped' }));
      await writeFile(join(sandbox, 'models.yml'), `providers:
  deck-smoke:
    baseUrl: http://127.0.0.1:${mock.address().port}/v1
    api: openai-completions
    auth: none
    models:
      - id: mock-local
        name: Deck Mock
        contextWindow: 128000
        maxTokens: 1024
`);
    }
    await click('Settings');
    await field('Custom executable or interpreter', binary);
    await click('Inspect');
    await field('Runtime name', `Smoke runtime ${suffix}-gjc-${version}`);
    await field('I trust this executable', true);
    await click('Register runtime');
    const installed = await eventually(async () => (await overview()).runtimes.find(item =>
      item.name === `Smoke runtime ${suffix}-gjc-${version}`), `GJC ${version} runtime`);
    assert.equal(installed.version, version);
    await click('Close dialog');
    await click('＋ New Session');
    await field('Runtime version', installed.id);
    await field('Session name', actualNames[index]);
    await field('Working directory', work);
    await evaluate(() => { document.querySelector('dialog details').open = true; });
    await field('Environment overrides', Object.entries(env).map(([key, value]) => `${key}=${value}`).join('\n'));
    if (checkRealMarkers) await field('Extra arguments', '--model\ndeck-smoke/mock-local');
    await click('Start session');
    await eventually(() => evaluate(() => {
      const text = document.querySelector('.terminal-pane:not([hidden]) .xterm-rows')?.textContent ?? '';
      return /gajae|gjc|credential|model|provider/i.test(text) && document.querySelector('footer')?.textContent.includes('no Agent status reported');
    }), `GJC ${version} interactive UI`);
    if (checkRealMarkers) {
      const rows = '.terminal-pane:not([hidden]) .xterm-rows';
      await eventually(() => evaluate(selector => /Deck Mock|mock-local/.test(document.querySelector(selector)?.textContent || ''), rows), 'selected local model');
      await terminalInput(version === '0.10.2' ? 'Deck queue timing probe' : 'Reply with a short status test message.');
      let firstFrame;
      try {
        if (version === '0.10.2') {
          await sleep(750);
          firstFrame = await evaluate(selector => document.querySelector(selector)?.innerText.split('\n').slice(-12), rows);
        }
        await waitText('footer', 'Work in progress');
        if (version === '0.10.2') {
          await send('Input.insertText', { text: 'unique queue draft 102' });
          await keypress('q', 'KeyQ', 81, 1);
          await waitText(rows, 'unique queue draft 102');
          await keypress('ArrowUp', 'ArrowUp', 38, 1);
          await send('Input.insertText', { text: ' recovered' });
          await waitText(rows, 'unique queue draft 102 recovered');
          await keypress('u', 'KeyU', 85, 2);
          console.log('GJC 0.10.2 Alt+Up restored a queued draft for editing.');
        }
        await waitText('.terminal-pane:not([hidden]) .xterm-rows', 'Deck loopback fixture reply');
        await waitText('footer', 'Agent run finished · check result');
        await terminalInput('Deck ask status probe');
        await waitText('.terminal-pane:not([hidden]) .xterm-rows', 'Which test option should Deck use?');
        await waitText('footer', 'Your response needed');
        await keypress('Enter', 'Enter', 13);
        await waitText('footer', 'Agent run finished · check result');
      } catch (error) {
        if (firstFrame) console.error({ firstFrame });
        console.error(await evaluate(() => ({
          status: document.querySelector('footer')?.textContent,
          terminal: document.querySelector('.terminal-pane:not([hidden]) .xterm-rows')?.innerText,
        })));
        throw error;
      }
      console.log(`GJC ${version} displayed working, ask attention, and finished in Deck using the local mock router.`);
      if (version === '0.10.2') continue;
      const original = (await overview()).sessions.find(item => item.name === actualNames[index]);
      assert(original);
      await stop();
      send = await start();
      await ready();
      await eventually(() => evaluate(name => {
        const row = Array.from(document.querySelectorAll('.session-item')).find(button => button.querySelector('strong')?.textContent === name);
        if (!row) return false;
        row.click();
        return true;
      }, original.name), 'original GJC session after app restart');
      await click('Resume Gajae-Code conversation');
      await waitText('dialog[open] h2', 'Resume conversation');
      const historyRoot = join(sandbox, 'sessions');
      const filesBefore = await conversationFiles(historyRoot);
      assert.equal(filesBefore.length, 1);
      const rowsBefore = (await overview()).sessions.map(session => session.id);
      await evaluate(() => { document.querySelector('dialog details').open = true; });
      await field('Environment overrides', Object.entries(env).map(([key, value]) => `${key}=${value}`).join('\n'));
      await field('Extra arguments', '--model\ndeck-smoke/mock-local');
      await click('Open conversation picker');
      await waitText(rows, 'Resume Session');
      await keypress('Enter', 'Enter', 13);
      await eventually(() => evaluate(selector => {
        const content = document.querySelector(selector)?.textContent ?? '';
        return content.includes('Resume this session?') || content.includes('Deck ask status probe');
      }, rows), 'native resume confirmation or restored conversation');
      if (await evaluate(selector => document.querySelector(selector)?.textContent.includes('Resume this session?'), rows)) {
        await keypress('Enter', 'Enter', 13);
      }
      await waitText(rows, 'Deck ask status probe');
      await terminalInput('Deck resumed context probe');
      await eventually(() => resumedContextSeen, 'follow-up uses restored conversation context');
      await eventually(async () => (await readFile(join(historyRoot, filesBefore[0]), 'utf8')).includes('Deck resumed context probe'), 'new message appended to the original conversation file');
      assert.deepEqual(await conversationFiles(historyRoot), filesBefore);
      assert.deepEqual((await overview()).sessions.map(session => session.id), rowsBefore);
      assert.equal(await evaluate(name => [...document.querySelectorAll('.session-row.selected strong')].filter(row => row.textContent === name).length, original.name), 1);
      console.log('GJC resume passed: continued the same conversation file and reused the Deck history row.');
    } else console.log(`GJC ${version} started its interactive TUI through Agent Deck. No inference requested.`);
  }
  }
  if (process.env.DECK_INSTALL_ONLY !== '1') {
    await presetSmoke();
    await checkInstallSettings();
  }
} finally {
  if (send && browser?.readyState === WebSocket.OPEN) {
    try {
      const data = await overview();
      for (const item of data.sessions.filter(item => [...sessionNames, ...actualNames].includes(item.name) || item.name.startsWith(presetPrefix))) {
        await invoke('close_terminal', { id: item.id });
        await invoke('delete_session', { id: item.id });
      }
      for (const item of data.runtimes.filter(item => item.name.startsWith(`Smoke runtime ${suffix}-`))) await invoke('delete_runtime', { id: item.id });
      for (const item of data.gjc_presets.filter(item => presetIds.includes(item.id))) {
        await invoke('delete_gjc_preset', { id: item.id, revision: item.revision });
      }
      for (const item of data.routers.filter(item => item.name === routerName || item.name.startsWith(presetPrefix))) await invoke('delete_router', { id: item.id });
      for (const item of data.roots.filter(item => item.path === root)) await invoke('delete_root', { id: item.id });
    } catch { /* Preserve the original smoke error. */ }
    try { await stop(); } catch { app?.kill(); }
  } else if (app?.exitCode === null) app.kill();
  mock.close();
  updatedRouter.close();
  await rm(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 }).catch(error => console.error(error.message));
}

