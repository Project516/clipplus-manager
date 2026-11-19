import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import { chromium } from 'playwright-core';
import { preview } from 'vite';

test('signs in, searches, downloads, and saves a song with a mock client', { timeout: 60_000 }, async (t) => {
  const server = await preview({ mode: 'fixture', preview: { host: '127.0.0.1', port: 0 } });
  t.after(() => server.close());
  const browser = await chromium.launch({ channel: 'chrome' });
  t.after(() => browser.close());
  const context = await browser.newContext();
  const page = await context.newPage();
  const origin = `http://127.0.0.1:${server.httpServer.address().port}`;
  const relayURL = 'wss://relay.invalid/socket';
  const healthURL = 'https://relay.invalid/healthz';

  await context.route('**/*', (route) => {
    const url = new URL(route.request().url());
    if (url.origin === origin) return route.continue();
    if (url.href === healthURL) return route.fulfill({ contentType: 'text/plain', body: 'ok' });
    return route.abort();
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

  const signIn = async (password) => {
    await page.getByLabel('Relay token').fill('fixture-token');
    await page.getByLabel('Soulseek username').fill('fixture-user');
    await page.getByLabel('Soulseek password').fill(password);
    await page.getByRole('button', { name: 'Sign in' }).click();
  };
  await signIn('wrong-password');
  await page.getByText('Soulseek rejected the sign-in: INVALIDPASS', { exact: true }).waitFor();
  assert.equal(await page.getByLabel('Relay token').inputValue(), '');
  assert.equal(await page.getByLabel('Soulseek password').inputValue(), '');

  await signIn('fixture-password');
  await page.getByText('Connected to Soulseek', { exact: true }).waitFor();
  assert.deepEqual(await page.evaluate(() => globalThis.nicotineMock.connections.at(-1)), {
    relayURL,
    relayToken: 'fixture-token',
    username: 'fixture-user',
  });

  await page.getByLabel('Search Soulseek').fill('song');
  await page.getByRole('button', { name: 'Search' }).click();
  await page.getByText('Found 4 files.', { exact: true }).waitFor();
  const rows = page.locator('#soulseek-results label');
  assert.deepEqual(await rows.locator('.file-name').allTextContents(), ['02 Free.mp3', '04 Missing.mp3', '01 Song.mp3', '03 Slow.flac']);
  const row = (name) => rows.filter({ hasText: name });
  assert.equal(await row('01 Song.mp3').locator('.file-details').textContent(), 'queue-user · 5.0 MB · 320 kbps · 3:45');
  const downloadButton = page.getByRole('button', { name: 'Download', exact: true });
  assert.equal(await downloadButton.isDisabled(), true);

  await row('01 Song.mp3').click();
  await downloadButton.click();
  const save = page.getByRole('link', { name: 'Save 01 Song.mp3' });
  await save.waitFor();
  const [saved] = await Promise.all([page.waitForEvent('download'), save.click()]);
  assert.equal(saved.suggestedFilename(), '01 Song.mp3');
  const song = await readFile(await saved.path());
  assert.equal(song.toString('latin1', 0, 4), 'ID3\x03', 'ID3v2.3 tag');
  assert.equal(song.includes('APIC'), false, 'cover art removed');
  assert.deepEqual(await page.evaluate(() => globalThis.nicotineMock.downloads.at(-1)), {
    user: 'queue-user',
    path: 'Music\\Album\\01 Song.mp3',
    size: 5_000_000,
  });

  await row('04 Missing.mp3').click();
  await downloadButton.click();
  await page.getByText('Download failed: The user is offline', { exact: true }).waitFor();

  await row('03 Slow.flac').click();
  await downloadButton.click();
  await page.getByText('Queued, position 2', { exact: true }).waitFor();
  assert.equal(await downloadButton.isDisabled(), true);
  await page.getByRole('button', { name: 'Cancel 03 Slow.flac' }).click();
  await page.getByText('Canceled', { exact: true }).waitFor();
  assert.equal(await downloadButton.isDisabled(), false);
  await downloadButton.click();
  await page.getByText('Queued, position 2', { exact: true }).waitFor();

  await page.evaluate(() => globalThis.nicotineMock.drop());
  await page.getByText('Canceled', { exact: true }).nth(1).waitFor();
  assert.equal(await page.locator('#soulseek-status').textContent(), 'Relay disconnected. Sign in to reconnect.');
  await page.getByRole('button', { name: 'Sign in' }).waitFor();
  await save.waitFor();
  await page.getByRole('button', { name: 'Clear finished' }).click();
  await page.getByRole('heading', { name: 'Downloads', level: 2 }).waitFor({ state: 'hidden' });

  await signIn('fixture-password');
  await page.getByRole('button', { name: 'Disconnect' }).click();
  await page.getByText('Disconnected.', { exact: true }).waitFor();
  assert.ok(await page.evaluate(() => globalThis.nicotineMock.closedByApp) >= 2);
});
