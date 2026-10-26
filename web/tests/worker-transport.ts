/** Fake transport for worker orchestration tests; CDN transport has its own caller tests. */
export async function mockVerification<T>(
  _project: string, keys: string[], _decrypt: (bytes: Uint8Array) => string,
  verify: (bytes: Uint8Array[], retry: boolean) => T | Promise<T>, _maximum?: number,
  read?: (key: string, direct: boolean) => Promise<Uint8Array>,
): Promise<T> {
  if (!read) throw new Error('The worker must supply its epoch-aware reader.');
  return verify(await Promise.all(keys.map(key=>read(key,false))),false);
}
