import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createProtocol } from '@project516/nicotine-plus-wasm';
import { createPeerLookup, secureRelayURL } from './soulseek.js';

test('accepts only secure relay URLs without embedded credentials', () => {
  assert.equal(secureRelayURL('wss://relay.invalid/socket').pathname, '/socket');
  for (const value of ['ws://relay.invalid', 'wss://user@relay.invalid', 'wss://relay.invalid/?key=fixture']) {
    assert.throws(() => secureRelayURL(value));
  }
});

test('encodes a search and looks up a peer using Soulseek frame fixtures', async (t) => {
  const protocol = await createProtocol();
  t.after(() => protocol.terminate());
  const bytes = (hex) => Uint8Array.from(Buffer.from(hex, 'hex'));

  assert.deepEqual(await protocol.encodeFileSearch(42, 'test'), bytes('100000001a0000002a0000000400000074657374'));
  assert.deepEqual(await protocol.decodeLogin(bytes('0d00000001000000000400000074657374')), {
    success: false,
    reason: 'test',
  });

  const incoming = [
    bytes('0400000020000000'),
    bytes('1a000000030000000400000070656572010200c0f9160000000000000000'),
  ];
  const sent = [];
  const request = Promise.withResolvers();
  const relay = {
    send(frame) {
      sent.push(frame);
      request.resolve();
    },
    async receive() {
      await request.promise;
      return incoming.shift() ?? new Promise(() => {});
    },
  };
  const lookupPeer = createPeerLookup(protocol, relay, assert.fail);
  assert.deepEqual(await lookupPeer('peer'), {
    username: 'peer',
    ipAddress: [192, 0, 2, 1].join('.'),
    port: 5881,
    obfuscatedPort: 0,
  });
  assert.deepEqual(sent, [bytes('0c000000030000000400000070656572')]);
});
