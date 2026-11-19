import assert from 'node:assert/strict';
import { test } from 'node:test';
import { chromium } from 'playwright-core';
import { preview } from 'vite';

test('signs in, sends a search, and shows a peer address through a relay fixture', { timeout: 60_000 }, async (t) => {
  const server = await preview({ preview: { host: '127.0.0.1', port: 0 } });
  t.after(() => server.close());
  const browser = await chromium.launch({ channel: 'chrome' });
  t.after(() => browser.close());
  const page = await browser.newPage();
  const relayURL = 'wss://relay.invalid/socket';
  const sent = [];
  const missingPeerRequest = Promise.withResolvers();

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

  await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/`);
  await page.getByRole('link', { name: 'Soulseek' }).click();
  await page.getByLabel('Private relay URL').fill(relayURL);
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
