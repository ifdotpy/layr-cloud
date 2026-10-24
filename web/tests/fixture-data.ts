import { readdirSync, readFileSync } from 'node:fs';

const directory = new URL('../fixtures/', import.meta.url);
export const fixtures: Record<string, unknown> = Object.fromEntries(
  readdirSync(directory).filter(name => name.endsWith('.json')).map(name => [
    name.replace(/\.json$/, ''), JSON.parse(readFileSync(new URL(name, directory), 'utf8')),
  ]),
);

export function fixture<T>(name: string, fallback?: T): T {
  if (Object.hasOwn(fixtures, name)) return fixtures[name] as T;
  if (fallback !== undefined) return fallback;
  throw new Error(`Required fixture is absent: ${name}.json`);
}
