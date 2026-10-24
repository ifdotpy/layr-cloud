import assert from 'node:assert/strict';
import { test } from 'node:test';
import { passkeySecret } from '../src/data/unlock';
import type { PersonView } from '../src/data/gateway';
Object.defineProperty(globalThis, 'location', { value: { hostname: 'layr.example' }, configurable: true });
const person: PersonView = { login: 'owner@example.com', recipient: 'public recipient', slots: [{ name: 'passkey-AQID', data: '' }], index: null, inbox: [] };

test('person passkey uses the exact Desk salt and returns the PRF bytes without derivation', async () => {
  const previous = Object.getOwnPropertyDescriptor(globalThis, 'navigator');
  const secret = new Uint8Array(32).fill(7);
  let requested: CredentialRequestOptions | undefined;
  Object.defineProperty(globalThis, 'navigator', { configurable: true, value: { credentials: { get: async (options: CredentialRequestOptions) => {
    requested = options;
    return { rawId: new Uint8Array([1, 2, 3]).buffer, getClientExtensionResults: () => ({ prf: { results: { first: secret.buffer } } }) };
  } } } });
  try {
    const result = await passkeySecret(person);
    const extensions = requested!.publicKey!.extensions as { prf: { eval: { first: Uint8Array } } };
    assert.equal(new TextDecoder().decode(extensions.prf.eval.first), 'layr-person/v1/owner@example.com');
    assert.equal(requested!.publicKey!.userVerification, 'required');
    assert.equal(requested!.publicKey!.rpId, 'layr.example');
    assert.equal(result.slot, 'passkey-AQID');
    assert.equal(result.secret.buffer, secret.buffer);
  } finally { if (previous) Object.defineProperty(globalThis, 'navigator', previous); }
});

test('a passkey provider without a PRF result cannot unlock a person', async () => {
  const previous = Object.getOwnPropertyDescriptor(globalThis, 'navigator');
  Object.defineProperty(globalThis, 'navigator', { configurable: true, value: { credentials: { get: async () => ({ rawId: new Uint8Array([1, 2, 3]).buffer, getClientExtensionResults: () => ({}) }) } } });
  try { await assert.rejects(passkeySecret(person), /no PRF result/); }
  finally { if (previous) Object.defineProperty(globalThis, 'navigator', previous); }
});
