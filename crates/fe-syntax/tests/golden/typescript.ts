// An LRU cache, used as a golden highlighting sample.

export interface Cache<K, V> {
  get(key: K): V | undefined;
  set(key: K, value: V): void;
  readonly size: number;
}

type Entry<V> = { value: V; hits: number };

export enum Policy {
  LeastRecent = 'lru',
  LeastFrequent = 'lfu',
}

const DEFAULT_CAPACITY = 256 as const;

export abstract class BaseCache<K, V> implements Cache<K, V> {
  protected readonly entries = new Map<K, Entry<V>>();

  constructor(protected capacity: number = DEFAULT_CAPACITY) {
    if (capacity <= 0) {
      throw new RangeError(`capacity must be positive, got ${capacity}`);
    }
  }

  get size(): number {
    return this.entries.size;
  }

  abstract evict(): void;

  get(key: K): V | undefined {
    const entry = this.entries.get(key);
    if (entry === undefined) return undefined;
    this.entries.delete(key);
    this.entries.set(key, { ...entry, hits: entry.hits + 1 });
    return entry.value;
  }

  set(key: K, value: V): void {
    this.entries.delete(key);
    while (this.entries.size >= this.capacity) this.evict();
    this.entries.set(key, { value, hits: 0 });
  }
}

export class LruCache<K, V> extends BaseCache<K, V> {
  override evict(): void {
    const oldest = this.entries.keys().next();
    if (!oldest.done) this.entries.delete(oldest.value);
  }
}

export function makeCache<K extends string | number, V>(policy: Policy = Policy.LeastRecent): Cache<K, V> {
  switch (policy) {
    case Policy.LeastRecent:
      return new LruCache<K, V>();
    default: {
      const unreachable: never = policy as never;
      throw new Error(String(unreachable));
    }
  }
}

declare const process: { env: Record<string, string | undefined> };
export const debug: boolean = process.env['CACHE_DEBUG'] === '1' && typeof window === 'undefined';
let hitRatio: number | null = null;
hitRatio = 0.75;
export { hitRatio };
