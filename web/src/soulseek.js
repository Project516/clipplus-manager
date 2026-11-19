export function secureRelayURL(value) {
  const url = new URL(value);
  if (url.protocol !== 'wss:' || url.username || url.password || url.search || url.hash) {
    throw new Error('Enter a secure WebSocket relay URL. Put the relay token in its own field.');
  }
  return url;
}

export async function lookupPeer(protocol, relay, username) {
  relay.send(await protocol.encodeGetPeerAddress(username));
  const deadline = Date.now() + 15000;
  while (Date.now() < deadline) {
    const frame = await relay.receive({ timeout: Math.max(1, deadline - Date.now()) });
    if (new DataView(frame.buffer, frame.byteOffset, frame.byteLength).getUint32(4, true) !== 3) continue;
    const peer = await protocol.decodeGetPeerAddress(frame);
    if (peer.username === username) return peer;
  }
  throw new Error('Peer address lookup timed out');
}
