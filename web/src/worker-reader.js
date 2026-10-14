// Only the worker holds decoded archive parts. Closing a vault terminates its worker.
export class WorkerReader {
  constructor(options) {
    this.worker = new Worker(new URL("./reader.worker.js", import.meta.url), {
      type: "module",
    });
    this.pending = new Map();
    this.sequence = 0;
    const trees = new Map();
    for (const key of options.keys) {
      const match = /\/(\d{10})\.([0-9a-f]{8})\.tree\.age$/.exec(key);
      if (match) {
        const seq = Number(match[1]);
        if (!trees.has(seq) || trees.get(seq) > match[2])
          trees.set(seq, match[2]);
      }
    }
    this.versions = [...trees]
      .sort((a, b) => a[0] - b[0])
      .map(([seq, attempt]) => ({ seq, attempt }));
    this.worker.onmessage = ({ data }) => {
      const pending = this.pending.get(data.id);
      if (!pending) return;
      if (data.progress) {
        pending.progress?.(data.progress);
        return;
      }
      this.pending.delete(data.id);
      if (data.error) pending.reject(new Error(data.error));
      else pending.resolve(data.result);
    };
    this.worker.onerror = () =>
      this.close("The file reader stopped. Unlock the vault again.");
    this.ready = this.call("init", options);
    // Keep initialization failures for the first caller without an unhandled rejection.
    this.ready.catch(() => {});
  }
  states() {
    return this.versions;
  }
  call(method, value, progress) {
    if (!this.worker) return Promise.reject(new Error("The vault is locked."));
    return new Promise((resolve, reject) => {
      const id = ++this.sequence;
      this.pending.set(id, { resolve, reject, progress });
      this.worker.postMessage({ id, method, value });
    });
  }
  async files(seq) {
    await this.ready;
    return this.call("files", seq);
  }
  async read(entry, progress) {
    await this.ready;
    return this.call("read", entry, progress);
  }
  close(message = "The vault is locked.") {
    this.worker?.terminate();
    this.worker = null;
    for (const pending of this.pending.values())
      pending.reject(new Error(message));
    this.pending.clear();
  }
}
