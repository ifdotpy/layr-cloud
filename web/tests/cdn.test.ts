import assert from 'node:assert/strict';
import {test} from 'node:test';
import {boundedBody, cacheable, signedLink} from '../src/data/cdn';
const id = '01a177aa-a7a5-716c-8774-7f165724c16b';
test('CDN classes exclude all mutable discovery and authority objects', () => {
  const keys = [`states/${id}/12345678.tree.age`, `states/${id}/12345678.0001.data.age`, `records/${id}/00000000000000000001-00000000000000000002.age`, `index/paths/${id}.age`, `index/graph/${id}.age`, `files/${id}.age`];
  for (const key of keys) { assert(cacheable(key), key); assert(cacheable(`project/epochs/${id}/${key}`)); }
  assert(cacheable(`project/epochs/${id}/history/root.age`));
  for (const key of ['store.json','public.json',`slots/${id}.age`,`keys/${id}.json`,`index/heads/${id}/00000000000000000001.age`,`requests/${id}.json`,`claims/${id}.json`,'records/a.age','history/root.age',`overlays/${id}/states/${id}/12345678.tree.age`]) assert(!cacheable(key), key);
});
test('signed links bind origin, path, expiry, size and exact standard parameters', () => {
  const origin = 'https://cloud.example', key = `files/${id}.age`, expires = Math.floor(Date.now()/1000)+300;
  const link = {url:`${origin}/o/project/${key}?Expires=${expires}&Signature=abc_~-&Key-Pair-Id=K123&Hash-Algorithm=SHA256`,expires,bytes:3};
  assert.deepEqual(signedLink(link,origin,'project',key,3), link);
  for (const url of [link.url.replace('https:', 'http:'),link.url.replace('cloud.example','other.example'),link.url.replace('/o/project/','/o/other/'),link.url.replace('files/','states/'),link.url+'#fragment',link.url+'&extra=1',link.url+`&Expires=${expires}`,link.url.replace('SHA256','SHA1'),link.url.replace('https://','https://user@')]) assert.throws(()=>signedLink({...link,url},origin,'project',key,3));
  assert.throws(()=>signedLink({...link,bytes:4},origin,'project',key,3));
  assert.throws(()=>signedLink({...link,expires:1},origin,'project',key,3));
});
test('ciphertext bounds are enforced on headers, streams and exact signed lengths', async () => {
  assert.deepEqual([...await boundedBody(new Response(new Uint8Array([1,2,3])),3,3)],[1,2,3]);
  await assert.rejects(boundedBody(new Response(new Uint8Array([1,2,3])),2),/limit/);
  await assert.rejects(boundedBody(new Response(new Uint8Array([1,2,3])),4,4),/length/);
  await assert.rejects(boundedBody(new Response('x',{headers:{'content-length':'5'}}),4),/limit/);
});
