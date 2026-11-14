// sheet-embed.1's shared vectors (Track S5): Cash Gear Workspace's
// packages/editor/vectors/sheet-embed-vectors.json, copied to tests/contracts with the commit it
// came from (tests/contracts/README.md). Sheet's validators must give every message the verdict
// the file gives it, as sent and after a structured clone, as Workspace's do.
import { describe, expect, it } from 'vitest';
import file from '../../../../tests/contracts/sheet-embed-vectors.json?raw';
import { CONTRACT, parseEmbedMessage, parseParentMessage } from './contract';

type Json = null | boolean | number | string | Json[] | { [key: string]: Json };

interface Vector {
  name: string;
  message: Json;
  valid: boolean;
}

interface Vectors {
  about: string;
  contract: string;
  parent: Vector[];
  embed: Vector[];
}

const vectors = JSON.parse(file) as Vectors;

/**
 * A vector's message as a page would post it: `{"$bytes": base64}` is an `ArrayBuffer` of those
 * bytes, and with `"$length": n` those bytes then zeros up to n. Keys are own properties, set
 * with `defineProperty`, so `__proto__` stays a key as a structured clone keeps it.
 */
function decode(value: Json): unknown {
  if (Array.isArray(value)) return value.map(decode);
  if (value === null || typeof value !== 'object') return value;
  const keys = Object.keys(value);
  if (
    typeof value.$bytes === 'string' &&
    keys.every((key) => key === '$bytes' || key === '$length')
  ) {
    const binary = atob(value.$bytes);
    const length = typeof value.$length === 'number' ? value.$length : binary.length;
    const bytes = new Uint8Array(length);
    for (let at = 0; at < Math.min(length, binary.length); at++) bytes[at] = binary.charCodeAt(at);
    return bytes.buffer;
  }
  const out: Record<string, unknown> = {};
  for (const key of keys) {
    Object.defineProperty(out, key, {
      value: decode(value[key]!),
      enumerable: true,
      writable: true,
      configurable: true,
    });
  }
  return out;
}

describe('sheet-embed-vectors.json', () => {
  it('is the contract this embed speaks, with cases both ways', () => {
    expect(vectors.contract).toBe(CONTRACT);
    expect(vectors.parent.length).toBeGreaterThan(0);
    expect(vectors.embed.length).toBeGreaterThan(0);
  });

  const suites = [
    ['parent', vectors.parent, parseParentMessage],
    ['embed', vectors.embed, parseEmbedMessage],
  ] as const;
  for (const [side, list, parse] of suites) {
    describe(side, () => {
      for (const vector of list) {
        it(`${vector.valid ? 'valid' : 'invalid'}: ${vector.name}`, () => {
          const message = decode(vector.message);
          expect(parse(message) !== null).toBe(vector.valid);
          expect(parse(structuredClone(message)) !== null).toBe(vector.valid);
        });
      }
    });
  }
});
