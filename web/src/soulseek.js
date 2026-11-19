export function secureRelayURL(value) {
  const url = new URL(value);
  if (url.protocol !== 'wss:' || url.username || url.password || url.search || url.hash) {
    throw new Error('Enter a secure WebSocket relay URL. Put the relay token in its own field.');
  }
  return url;
}

export function createPeerLookup(protocol, relay, onDisconnect) {
  let pending;

  async function receive() {
    for (;;) {
      const frame = await relay.receive();
      if (!pending || new DataView(frame.buffer, frame.byteOffset, frame.byteLength).getUint32(4, true) !== 3) continue;
      const lookup = pending;
      try {
        const peer = await protocol.decodeGetPeerAddress(frame);
        if (peer.username === lookup.username) lookup.resolve(peer);
      } catch (error) {
        lookup.reject(error);
      }
    }
  }

  receive().catch((error) => {
    pending?.reject(error);
    onDisconnect();
  });

  return async (username) => {
    const frame = await protocol.encodeGetPeerAddress(username);
    const { promise, resolve, reject } = Promise.withResolvers();
    pending = { username, resolve, reject };
    // The package's receive timeout closes the relay.
    const timeout = setTimeout(() => resolve(null), 15000);
    try {
      relay.send(frame);
      return await promise;
    } finally {
      clearTimeout(timeout);
      pending = undefined;
    }
  };
}
