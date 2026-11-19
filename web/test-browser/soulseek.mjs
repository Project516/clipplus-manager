import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createRequire } from 'node:module';
import { readFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { chromium } from 'playwright-core';
import { preview } from 'vite';

const require = createRequire(import.meta.url);
const pyodidePath = dirname(createRequire(require.resolve('@project516/nicotine-plus-wasm')).resolve('pyodide'));
const pyodideRoot = 'https://cdn.jsdelivr.net/pyodide/v314.0.7/full/';
const runtimeFiles = new Map([
  ['pyodide.mjs', 'text/javascript'],
  ['pyodide.asm.mjs', 'text/javascript'],
  ['pyodide.asm.wasm', 'application/wasm'],
  ['pyodide-lock.json', 'application/json'],
  ['python_stdlib.zip', 'application/zip'],
]);

test('signs in, sends a search, and shows a peer address through a relay fixture', { timeout: 60_000 }, async (t) => {
  const server = await preview({ preview: { host: '127.0.0.1', port: 0 } });
  t.after(() => server.close());
  const browser = await chromium.launch({ channel: 'chrome' });
  t.after(() => browser.close());
  const context = await browser.newContext();
  const page = await context.newPage();
  const origin = `http://127.0.0.1:${server.httpServer.address().port}`;
  const relayURL = 'wss://relay.invalid/socket';
  const healthURL = 'https://relay.invalid/healthz';
  const sent = [];
  const missingPeerRequest = Promise.withResolvers();

  await context.route('**/*', async (route) => {
    const url = new URL(route.request().url());
    if (url.origin === origin) return route.continue();
    if (url.href === healthURL) return route.fulfill({ contentType: 'text/plain', body: 'ok' });
    if (url.href.startsWith(pyodideRoot)) {
      const file = url.href.slice(pyodideRoot.length);
      const contentType = runtimeFiles.get(file);
      if (contentType) {
        return route.fulfill({
          body: await readFile(join(pyodidePath, file)),
          contentType,
          headers: { 'Access-Control-Allow-Origin': '*' },
        });
      }
    }
    return route.abort();
  });

  await page.routeWebSocket(relayURL, (socket) => {
    socket.onMessage((message) => {
      sent.push(message);
      if (typeof message === 'string') {
        socket.send('ready');
        return;
      }
      switch (message.readUInt32LE(4)) {
        case 1:
          socket.send(Buffer.from('12000000010000000100000000010000c00000000000', 'hex'));
          break;
        case 3:
          if (message.toString('utf8', 12) === 'missing') {
            missingPeerRequest.resolve();
            break;
          }
          socket.send(Buffer.from('1a000000030000000400000070656572010200c0f9160000000000000000', 'hex'));
          break;
      }
    });
  });

  await page.goto(origin);
  await page.getByRole('link', { name: 'Soulseek' }).click();
  const relayInput = page.getByLabel('Private relay URL');
  await relayInput.fill(relayURL);
  const healthPagePromise = page.waitForEvent('popup');
  await page.getByRole('link', { name: 'sign in to the relay in a new tab' }).click();
  const healthPage = await healthPagePromise;
  await healthPage.getByText('ok', { exact: true }).waitFor();
  assert.equal(healthPage.url(), healthURL);
  await healthPage.close();
  await page.reload();
  assert.equal(await relayInput.inputValue(), relayURL);
  await page.getByLabel('Relay token').fill('fixture-token');
  await page.getByLabel('Soulseek username').fill('fixture-user');
  await page.getByLabel('Soulseek password').fill('fixture-password');
  await page.getByRole('button', { name: 'Sign in' }).click();
  await page.getByText('Signed in to Soulseek.', { exact: true }).waitFor();
  assert.equal(sent[0], 'fixture-token');
  assert.equal(sent[1].readUInt32LE(4), 1);
  assert.equal(sent[1].toString('utf8', 12, 24), 'fixture-user');
  assert.equal(await page.getByLabel('Soulseek password').inputValue(), '');
  assert.equal(await page.getByLabel('Relay token').inputValue(), '');

  await page.getByLabel('Search query').fill('test');
  await page.getByRole('button', { name: 'Send search request' }).click();
  await page.getByText('Search request sent. Search results are unavailable.', { exact: true }).waitFor();
  await page.getByLabel('Peer username').fill('peer');
  await page.getByRole('button', { name: 'Look up peer address' }).click();
  await page.getByText(`peer · ${[192, 0, 2, 1].join('.')}:5881`, { exact: true }).waitFor();
  assert.equal(sent[2].readUInt32LE(4), 26);
  assert.equal(sent[2].toString('utf8', 16), 'test');
  assert.equal(sent[3].toString('hex'), '0c000000030000000400000070656572');

  await page.clock.install();
  await page.getByLabel('Peer username').fill('missing');
  await page.getByRole('button', { name: 'Look up peer address' }).click();
  await missingPeerRequest.promise;
  await page.clock.fastForward(15_001);
  await page.getByText('The peer address lookup timed out.', { exact: true }).waitFor();

  await page.getByLabel('Peer username').fill('peer');
  await page.getByRole('button', { name: 'Look up peer address' }).click();
  await page.getByText(`peer · ${[192, 0, 2, 1].join('.')}:5881`, { exact: true }).waitFor();

  await page.getByLabel('Peer username').fill('missing');
  await page.getByRole('button', { name: 'Look up peer address' }).click();
  await page.getByText('Looking up peer address…', { exact: true }).waitFor();
  await page.getByRole('button', { name: 'Disconnect' }).click();
  await page.getByText('Disconnected.', { exact: true }).waitFor();
  await page.getByRole('button', { name: 'Sign in' }).waitFor();
});
