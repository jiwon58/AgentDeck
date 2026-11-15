import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { copyFile, mkdir, mkdtemp, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import { setTimeout as sleep } from 'node:timers/promises';

const application = resolve(process.argv[2] ?? 'target/release/agent-deck.exe');
const fixture = resolve(process.argv[3] ?? 'target/release/mock-agent.exe');
const root = await mkdtemp(join(tmpdir(), 'agent-deck-smoke-'));
let resumedContextSeen = false;
const mock = createServer((request, response) => {
  if (request.method === 'POST' && request.url === '/v1/chat/completions') {
    let body = '';
    request.on('data', chunk => { body += chunk; });
    request.once('end', () => {
      const messages = JSON.parse(body).messages;
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
      }, 1500);
    });
    return;
  }
  if (request.url !== '/v1/models') { response.writeHead(404).end(); return; }
  response.writeHead(200, { 'Content-Type': 'application/json' }).end('{"data":[{"id":"mock-local"}]}');
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
const resumeName = `Smoke resumed GJC ${suffix}`;
const routerName = `Mock router ${suffix}`;
const fakeToken = `deck-diagnostic-test-token-${suffix}`;
const updatedRouter = createServer();
await new Promise(resolve => updatedRouter.listen(0, '127.0.0.1', resolve));
let app;
let browser;

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
    if (!button || button.disabled) return false;
    button.click();
    return true;
  }, text, index), `button ${text}`);
}
async function field(labelText, value) {
  await eventually(() => evaluate((labelText, value) => {
    const scope = document.querySelector('dialog[open]') ?? document;
    const label = Array.from(scope.querySelectorAll('label')).find(label =>
      label.getClientRects().length && label.textContent.trim().startsWith(labelText));
    const input = label?.querySelector('input,select,textarea');
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
const invoke = (name, data) => evaluate((name, data) => window.__TAURI_INTERNALS__.invoke(name, data), name, data);
const overview = () => invoke('overview', {});
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
async function stop() {
  const closed = new Promise(resolve => app.once('exit', resolve));
  try { await invoke('quit', {}); } catch { /* The WebView can close before the reply. */ }
  await Promise.race([closed, sleep(8000).then(() => { if (app.exitCode === null) throw new Error('Application did not close'); })]);
  browser.close();
}

try {
  send = await start();
  await ready();
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
    const checkRealMarkers = version === '0.20.0' && process.env.DECK_GJC_CHECK_STATUS === '1';
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
      await waitText('.terminal-pane:not([hidden]) .xterm-rows', 'Deck Mock');
      await terminalInput('Reply with a short status test message.');
      try {
        await waitText('footer', 'Work in progress');
        await waitText('.terminal-pane:not([hidden]) .xterm-rows', 'Deck loopback fixture reply');
        await waitText('footer', 'Agent run finished · check result');
        await terminalInput('Deck ask status probe');
        await waitText('.terminal-pane:not([hidden]) .xterm-rows', 'Which test option should Deck use?');
        await waitText('footer', 'Your response needed');
        await keypress('Enter', 'Enter', 13);
        await waitText('footer', 'Agent run finished · check result');
      } catch (error) {
        console.error(await evaluate(() => ({
          status: document.querySelector('footer')?.textContent,
          terminal: document.querySelector('.terminal-pane:not([hidden]) .xterm-rows')?.innerText,
        })));
        throw error;
      }
      console.log(`GJC ${version} emitted working, ask attention, and finished events through ConPTY using the local mock router.`);
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
      await field('Session name', resumeName);
      await evaluate(() => { document.querySelector('dialog details').open = true; });
      await field('Environment overrides', Object.entries(env).map(([key, value]) => `${key}=${value}`).join('\n'));
      await field('Extra arguments', '--model\ndeck-smoke/mock-local');
      await click('Open conversation picker');
      const rows = '.terminal-pane:not([hidden]) .xterm-rows';
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
      const resumed = (await overview()).sessions.find(item => item.name === resumeName);
      assert.equal(resumed?.resume_source_id, original.id);
      assert.notEqual(resumed.id, original.id);
      await terminalInput('Deck resumed context probe');
      await eventually(() => resumedContextSeen, 'follow-up uses restored conversation context');
      console.log('GJC resume passed after Deck restart: native history picker restored the prior conversation in a new terminal.');
    } else console.log(`GJC ${version} started its interactive TUI through Agent Deck. No inference requested.`);
  }
} finally {
  if (send && browser?.readyState === WebSocket.OPEN) {
    try {
      const data = await overview();
      for (const item of data.sessions.filter(item => [...sessionNames, ...actualNames, resumeName].includes(item.name))) {
        await invoke('close_terminal', { id: item.id });
        await invoke('delete_session', { id: item.id });
      }
      for (const item of data.runtimes.filter(item => item.name.startsWith(`Smoke runtime ${suffix}-`))) await invoke('delete_runtime', { id: item.id });
      for (const item of data.routers.filter(item => item.name === routerName)) await invoke('delete_router', { id: item.id });
      for (const item of data.roots.filter(item => item.path === root)) await invoke('delete_root', { id: item.id });
    } catch { /* Preserve the original smoke error. */ }
    try { await stop(); } catch { app?.kill(); }
  } else if (app?.exitCode === null) app.kill();
  mock.close();
  updatedRouter.close();
  await rm(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 }).catch(error => console.error(error.message));
}

