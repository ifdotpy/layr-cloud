/** Transport hints only. The caller must still verify age, signatures and file hashes. */
const uuid = '[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}';
const immutable = new RegExp(`^(?:states/${uuid}/[a-f0-9]{8}\\.(?:tree|[0-9]{4}\\.data)\\.age|records/${uuid}/[0-9]{20}-[0-9]{20}\\.age|index/(?:paths|graph)/${uuid}\\.age|files/${uuid}\\.age)$`);
export function cacheable(key: string): boolean {
  const epoch = new RegExp(`^project/epochs/${uuid}/`).exec(key);
  const logical = epoch ? key.slice(epoch[0].length) : key;
  return immutable.test(logical) || (!!epoch && new RegExp(`^history/(?:root|${uuid})\\.age$`).test(logical));
}
export interface CdnLink {url: string; expires: number; bytes: number}
export function signedLink(value: unknown, origin: string, project: string, key: string, maximum: number): CdnLink {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('Invalid CDN reply.');
  const link = value as CdnLink;
  if (Object.keys(link).sort().join(',') !== 'bytes,expires,url' || typeof link.url !== 'string'
    || !Number.isSafeInteger(link.bytes) || link.bytes < 0 || link.bytes > maximum
    || !Number.isSafeInteger(link.expires) || link.expires <= Date.now() / 1000 || link.expires > Date.now() / 1000 + 600) throw new Error('Invalid CDN reply.');
  const url = new URL(link.url), names = [...url.searchParams.keys()];
  if (url.origin !== origin || url.protocol !== 'https:' || url.username || url.password || url.hash
    || url.pathname !== `/o/${project}/${key}` || names.length !== 4 || new Set(names).size !== 4
    || url.searchParams.get('Hash-Algorithm') !== 'SHA256' || url.searchParams.get('Expires') !== String(link.expires)
    || !/^[A-Za-z0-9]{1,128}$/.test(url.searchParams.get('Key-Pair-Id') || '')
    || !/^[A-Za-z0-9_~-]{1,1024}$/.test(url.searchParams.get('Signature') || '')) throw new Error('Invalid CDN URL.');
  return link;
}
export async function boundedBody(response: Response, maximum: number, expected?: number): Promise<Uint8Array> {
  const length = response.headers.get('content-length');
  if (length !== null && (!/^\d+$/.test(length) || Number(length) > maximum || (expected !== undefined && Number(length) !== expected))) throw new Error('Ciphertext exceeds its size limit or has a different length.');
  const reader = response.body?.getReader();
  if (!reader) throw new Error('Ciphertext response has no body.');
  const chunks: Uint8Array[] = []; let total = 0;
  try {
    for (;;) {
      const {done, value} = await reader.read(); if (done) break;
      total += value.byteLength;
      if (total > maximum) throw new Error('Ciphertext exceeds its size limit.');
      chunks.push(value);
    }
    if (expected !== undefined && total !== expected) throw new Error('Ciphertext length differs.');
  } catch (error) { await reader.cancel().catch(() => {}); throw error; }
  finally { reader.releaseLock(); }
  const bytes = new Uint8Array(total); let offset = 0;
  for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; }
  return bytes;
}
