import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
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

test('the production build connects from saved settings and sends a search through a relay fixture', { timeout: 60_000 }, async (t) => {
  const server = await preview({ preview: { host: '127.0.0.1', port: 0 } });
  t.after(() => server.close());
  const browser = await chromium.launch({ channel: 'chrome' });
  t.after(() => browser.close());
  const context = await browser.newContext();
  const page = await context.newPage();
  const origin = `http://127.0.0.1:${server.httpServer.address().port}`;
  const relayURL = 'wss://relay.invalid/relay';
  const sent = [];
  const searched = Promise.withResolvers();

  await context.route('**/*', async (route) => {
    const url = new URL(route.request().url());
    if (url.origin === origin) return route.continue();
    const contentType = url.href.startsWith(pyodideRoot) && runtimeFiles.get(url.href.slice(pyodideRoot.length));
    if (contentType) {
      return route.fulfill({
        body: await readFile(join(pyodidePath, url.href.slice(pyodideRoot.length))),
        contentType,
        headers: { 'Access-Control-Allow-Origin': '*' },
      });
    }
    if (url.origin === 'https://musicbrainz.org') {
      const releases = [{ title: 'Query', 'track-count': 1, 'artist-credit': [{ name: 'Fixture' }], 'release-group': { id: 'fixture', title: 'Query', 'primary-type': 'Album' } }];
      return route.fulfill({ contentType: 'application/json', headers: { 'Access-Control-Allow-Origin': '*' }, body: JSON.stringify({ releases }) });
    }
    return route.abort();
  });

  await page.routeWebSocket(relayURL, (socket) => {
    socket.onMessage((message) => {
      sent.push(message);
      if (typeof message === 'string') return socket.send('ready');
      const code = message.readUInt32LE(4);
      if (code === 1) socket.send(Buffer.from('12000000010000000100000000010000c00000000000', 'hex'));
      if (code === 26) searched.resolve(message);
    });
  });

  await page.goto(`${origin}/#settings`);
  await page.getByLabel('Relay URL').fill(relayURL);
  await page.getByLabel('Relay token').fill('fixture-token');
  await page.getByLabel('Soulseek username').fill('fixture-user');
  await page.getByLabel('Soulseek password').fill('fixture-password');
  await page.locator('#relay-form').getByRole('button', { name: 'Save settings' }).click();
  await page.getByText('Settings saved.', { exact: true }).waitFor();
  await page.getByRole('navigation').getByRole('link', { name: 'Soulseek' }).click();
  await page.getByText('Connected to Soulseek', { exact: true }).waitFor();
  assert.equal(sent[0], 'fixture-token');
  assert.equal(sent[1].readUInt32LE(4), 1);
  assert.equal(sent[1].toString('utf8', 12, 24), 'fixture-user');

  await page.getByLabel('Search for').selectOption('Album');
  await page.getByLabel('Artist and album').fill('fixture query');
  await page.getByRole('button', { name: 'Search', exact: true }).click();
  await page.locator('#soulseek-songs label').first().click();
  await page.getByRole('button', { name: 'Find downloads' }).click();
  const search = await searched.promise;
  assert.equal(search.toString('utf8', 16), 'Fixture Query');

  await page.getByRole('button', { name: 'Back' }).click();
  await page.getByRole('button', { name: 'Disconnect' }).click();
  await page.getByText('Disconnected.', { exact: true }).waitFor();
});
