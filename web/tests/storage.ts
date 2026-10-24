import { expect, type Page } from '@playwright/test';

declare global {
  interface Window { __persistenceWrites: string[]; __persistenceObserverReady: boolean }
}

/** Detect writes even when the application later removes the saved value. */
export async function observePersistence(page: Page) {
  // A string prevents a TypeScript runner from inserting helpers into browser code.
  await page.addInitScript({ content: `(() => {
    window.__persistenceObserverReady = false;
    window.__persistenceWrites = [];
    const record = name => window.__persistenceWrites.push(name);
    const setItem = Storage.prototype.setItem;
    Storage.prototype.setItem = function (...args) {
      record(this === localStorage ? 'localStorage.setItem' : 'sessionStorage.setItem');
      return setItem.apply(this, args);
    };
    for (const method of ['add', 'put']) {
      const original = IDBObjectStore.prototype[method];
      IDBObjectStore.prototype[method] = function (...args) {
        record('IndexedDB.' + method);
        return original.apply(this, args);
      };
    }
    const cookie = Object.getOwnPropertyDescriptor(Document.prototype, 'cookie');
    if (cookie?.get && cookie.set) {
      Object.defineProperty(Document.prototype, 'cookie', {
        configurable: cookie.configurable,
        enumerable: cookie.enumerable,
        get: cookie.get,
        set(value) {
          record('cookie');
          cookie.set.call(this, value);
        },
      });
    }
    window.__persistenceObserverReady = true;
  })();` });
}

export async function expectNoPersistence(page: Page, name: string) {
  const storage = await page.evaluate(async () => ({
    observerReady: window.__persistenceObserverReady,
    localItems: localStorage.length,
    sessionItems: sessionStorage.length,
    databases: (await indexedDB.databases()).length,
    writes: window.__persistenceWrites,
  }));
  expect(storage, `${name}: browser storage must stay empty`).toEqual({
    observerReady: true,
    localItems: 0,
    sessionItems: 0,
    databases: 0,
    writes: [],
  });
  expect(await page.context().cookies(), `${name}: cookies must stay empty`).toEqual([]);
}
