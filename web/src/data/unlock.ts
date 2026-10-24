import type { PersonView } from './gateway';
const decodeId = (value: string) => Uint8Array.from(atob(value.replace(/-/g, '+').replace(/_/g, '/')), c => c.charCodeAt(0));
const encodeId = (value: ArrayBuffer) => btoa(String.fromCharCode(...new Uint8Array(value))).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
export interface PasskeyResult { slot: string; secret: Uint8Array }
/** WebAuthn must run in the page. Transfer its PRF bytes to the worker immediately. */
export async function passkeySecret(person: PersonView): Promise<PasskeyResult> {
  // Exact Desk person PRF salt, from js/vault/src/person.mjs at 31610ce.
  const salt = new TextEncoder().encode(`layr-person/v1/${person.login}`);
  const slots = person.slots.filter(slot => slot.name.startsWith('passkey-'));
  if (!slots.length) throw new Error('No passkey opens your person key.');
  const credential = await navigator.credentials.get({ publicKey: {
    challenge: crypto.getRandomValues(new Uint8Array(32)),
    rpId: location.hostname,
    userVerification: 'required',
    allowCredentials: slots.map(slot => ({ type: 'public-key' as const, id: decodeId(slot.name.slice(8)) })),
    extensions: { prf: { eval: { first: salt } } } as AuthenticationExtensionsClientInputs,
  } }) as PublicKeyCredential | null;
  if (!credential) throw new Error('The passkey request has no result.');
  const result = credential.getClientExtensionResults() as AuthenticationExtensionsClientOutputs & { prf?: { results?: { first?: ArrayBuffer } } };
  const secret = result.prf?.results?.first;
  if (!secret) throw new Error('This passkey provider gives no PRF result.');
  const slot = `passkey-${encodeId(credential.rawId)}`;
  if (!slots.some(item => item.name === slot)) throw new Error('The passkey has no person slot.');
  return { slot, secret: new Uint8Array(secret) };
}
