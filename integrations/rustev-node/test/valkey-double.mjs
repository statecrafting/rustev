// Spec 063 3.7: a reference Redis-shaped store test double, in memory, with
// no network access and no wall-clock time. It models what a host's store
// needs from Valkey 8.1: string and hash keys, PEXPIRE with lazy expiry on an
// injected clock, `maxmemory` with `maxmemory-policy volatile-lru` (only keys
// with a TTL are evictable; a write that cannot make room is refused with
// OOM, as Valkey refuses it), Lua-script atomicity (a script runs with no
// interleaving), and a primary with an asynchronously replicated reader.
//
// `ValkeyStore` is the `RuntimeStore` the spec's Redis table describes: keys
// under one hash tag per namespace, `get`, `put` and `delete` each one
// script on the primary, `invalidate` an INCR of a generation key with no
// TTL. Its options produce the non-conforming variants section 5 lists, so
// tests can show which failures Rustev detects and which it cannot.

/** One Valkey node's keyspace. */
export class ValkeyNode {
  constructor({ maxmemory = Infinity, policy = 'volatile-lru', now = () => 0 } = {}) {
    this.maxmemory = maxmemory;
    this.policy = policy;
    this.now = now;
    this.keys = new Map();
    this.tick = 0;
    this.evicted = [];
  }

  #live(k) {
    const e = this.keys.get(k);
    if (e && e.expiresAt != null && e.expiresAt <= this.now()) {
      this.keys.delete(k);
      return undefined;
    }
    if (e) e.lru = ++this.tick;
    return e;
  }

  #size(k, e) {
    const v = e.type === 'hash' ? [...e.value.entries()].reduce((n, [f, x]) => n + f.length + x.length, 0) : e.value.length;
    return k.length + v + 16;
  }

  used() {
    let n = 0;
    for (const [k, e] of this.keys) n += this.#size(k, e);
    return n;
  }

  /** Store `entry` under `k` only if room can be made, as Valkey does:
   * evict by the policy (never `k` itself), or refuse the write with OOM
   * when nothing evictable is left, leaving the keyspace as it was. */
  #write(k, entry) {
    const old = this.keys.get(k);
    this.keys.set(k, entry);
    while (this.used() > this.maxmemory) {
      const candidates = [...this.keys].filter(([key, e]) => key !== k && (this.policy === 'allkeys-lru' || e.expiresAt != null));
      if (candidates.length === 0) {
        if (old) this.keys.set(k, old);
        else this.keys.delete(k);
        throw new Error('OOM command not allowed when used memory > maxmemory');
      }
      candidates.sort((a, b) => a[1].lru - b[1].lru);
      this.keys.delete(candidates[0][0]);
      this.evicted.push(candidates[0][0]);
    }
  }

  get(k) {
    const e = this.#live(k);
    return e?.type === 'string' ? e.value : null;
  }
  set(k, v) {
    const old = this.keys.get(k);
    this.#write(k, { type: 'string', value: Buffer.from(String(v)), expiresAt: old?.expiresAt ?? null, lru: ++this.tick });
  }
  incrby(k, by) {
    const n = Number(this.get(k) ?? 0) + by;
    this.set(k, String(n));
    return n;
  }
  hget(k, f) {
    const e = this.#live(k);
    return e?.type === 'hash' ? (e.value.get(f) ?? null) : null;
  }
  hset(k, fields) {
    const cur = this.#live(k);
    // A copy, so a refused write leaves the old hash untouched.
    const e = { type: 'hash', value: new Map(cur?.value ?? []), expiresAt: cur?.expiresAt ?? null, lru: ++this.tick };
    for (const [f, v] of Object.entries(fields)) e.value.set(f, Buffer.from(v));
    this.#write(k, e);
  }
  exists(k) {
    return this.#live(k) !== undefined;
  }
  del(k) {
    return this.keys.delete(k) ? 1 : 0;
  }
  pexpire(k, ms) {
    const e = this.#live(k);
    if (e) e.expiresAt = this.now() + ms;
  }
  /** A copy of the keyspace, as a replica holds it after a full sync. */
  snapshot() {
    const copy = new Map();
    for (const [k, e] of this.keys) {
      copy.set(k, { ...e, value: e.type === 'hash' ? new Map(e.value) : Buffer.from(e.value) });
    }
    return copy;
  }
}

/** A primary and one reader endpoint that receives the primary's keyspace
 * only when `replicate()` runs: replication lag made explicit. */
export class ValkeyCluster {
  constructor(options) {
    this.primary = new ValkeyNode(options);
    this.replica = new ValkeyNode(options);
  }
  replicate() {
    this.replica.keys = this.primary.snapshot();
  }
  /** A Lua script: runs to completion with no other command in between. */
  eval(node, script) {
    return script(node);
  }
}

const tag = (ns) => `{rx:${ns}}`;
const gen = (v) => Number(v ?? 0);
const yieldNow = () => new Promise((r) => setImmediate(r));

/**
 * The `RuntimeStore` of spec 063 3.7 over a `ValkeyCluster`.
 * Options:
 *  - `entryTtlMs`: each entry's TTL (at least the plan's `ttl_ms`).
 *  - `readFrom: 'replica'`: reads from the reader endpoint (breaks G2).
 *  - `atomicGet: false`: reads the entry and the generations in separate
 *    round trips, with `between()` awaited in the gap (breaks G3).
 *  - `atomicPut: false`: checks the generations, awaits `between()`, then
 *    writes (breaks G4).
 *  - `removeOnInvalidate: false`: leaves covered entries in place after the
 *    advance. G2 allows it, since a stale entry fails the generation check,
 *    but under first-writer-wins a stale entry also answers every later
 *    `put` of its key with `conflict`, so the key stays a miss until the
 *    entry's TTL reclaims it.
 * `calls` counts each method.
 */
export class ValkeyStore {
  constructor(cluster, contract, { entryTtlMs = 3_600_000, readFrom = 'primary', atomicGet = true, atomicPut = true, removeOnInvalidate = true, between = async () => {} } = {}) {
    this.cluster = cluster;
    this.contract = contract;
    this.queueCapacity = 64;
    this.options = { entryTtlMs, readFrom, atomicGet, atomicPut, removeOnInvalidate, between };
    this.calls = { get: 0, put: 0, delete: 0, invalidate: 0 };
  }

  #reader() {
    return this.options.readFrom === 'replica' ? this.cluster.replica : this.cluster.primary;
  }

  async get({ namespace, scope, keyId }) {
    this.calls.get++;
    const t = tag(namespace);
    const read = (n) => ({
      bytes: n.hget(`${t}:e:${keyId}`, 'bytes'),
      generations: { namespace: gen(n.get(`${t}:g`)), scope: gen(n.get(`${t}:g:${scope}`)) },
    });
    const node = this.#reader();
    if (this.options.atomicGet) {
      const r = this.cluster.eval(node, read);
      return { bytes: r.bytes == null ? null : new Uint8Array(r.bytes), generations: r.generations };
    }
    const bytes = node.hget(`${t}:e:${keyId}`, 'bytes');
    await this.options.between('get');
    const generations = { namespace: gen(node.get(`${t}:g`)), scope: gen(node.get(`${t}:g:${scope}`)) };
    return { bytes: bytes == null ? null : new Uint8Array(bytes), generations };
  }

  async put({ namespace, keyId, bytes, terms }) {
    this.calls.put++;
    const t = tag(namespace);
    const p = this.cluster.primary;
    const current = (n) => ({ namespace: gen(n.get(`${t}:g`)), scope: gen(n.get(`${t}:g:${terms.scope}`)) });
    const same = (g) => g.namespace === terms.generations.namespace && g.scope === terms.generations.scope;
    const write = (n) => {
      if (n.exists(`${t}:e:${keyId}`)) return 'conflict';
      const len = bytes.length;
      if (gen(n.get(`${t}:b:${terms.scope}`)) + len > terms.maxScopeBytes) return 'over_budget';
      if (gen(n.get(`${t}:b`)) + len > terms.maxNamespaceBytes) return 'over_budget';
      n.hset(`${t}:e:${keyId}`, { bytes: Buffer.from(bytes), scope: terms.scope });
      n.pexpire(`${t}:e:${keyId}`, this.options.entryTtlMs);
      n.incrby(`${t}:b:${terms.scope}`, len);
      n.incrby(`${t}:b`, len);
      return 'stored';
    };
    if (this.options.atomicPut) {
      return this.cluster.eval(p, (n) => (same(current(n)) ? write(n) : 'generation_advanced'));
    }
    if (!same(current(p))) return 'generation_advanced';
    await this.options.between('put');
    return write(p);
  }

  async delete({ namespace, keyId }) {
    this.calls.delete++;
    const t = tag(namespace);
    this.cluster.eval(this.cluster.primary, (n) => {
      const bytes = n.hget(`${t}:e:${keyId}`, 'bytes');
      const scope = n.hget(`${t}:e:${keyId}`, 'scope');
      if (bytes == null) return;
      n.incrby(`${t}:b:${scope}`, -bytes.length);
      n.incrby(`${t}:b`, -bytes.length);
      n.del(`${t}:e:${keyId}`);
    });
  }

  /** G1, G2: the INCR is the invalidation, atomic alone; removing the
   * covered entries runs afterwards, one script, and is optional. */
  async invalidate({ namespace, selector }) {
    this.calls.invalidate++;
    const t = tag(namespace);
    const p = this.cluster.primary;
    const key = selector === 'namespace' ? `${t}:g` : `${t}:g:${selector.scope}`;
    const generation = p.incrby(key, 1);
    await yieldNow();
    if (!this.options.removeOnInvalidate) return { generation, removed: 0 };
    const removed = this.cluster.eval(p, (n) => {
      let count = 0;
      for (const k of [...n.keys.keys()].filter((k) => k.startsWith(`${t}:e:`))) {
        const scope = n.hget(k, 'scope');
        const bytes = n.hget(k, 'bytes');
        if (bytes == null || (selector !== 'namespace' && String(scope) !== selector.scope)) continue;
        n.incrby(`${t}:b:${scope}`, -bytes.length);
        n.incrby(`${t}:b`, -bytes.length);
        n.del(k);
        count++;
      }
      return count;
    });
    return { generation, removed };
  }
}
