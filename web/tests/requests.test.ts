import assert from 'node:assert/strict';
import test from 'node:test';
import { uuidV7 } from '../src/data/requests';
test('request IDs preserve 48-bit time, UUIDv7 version, variant, and unique randomness', () => {
  const values = new Set(Array.from({length:1000}, () => uuidV7(0x123456789abc)));
  assert.equal(values.size,1000);
  for (const value of values) assert.match(value,/^12345678-9abc-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
});
