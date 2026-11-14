// Reading `init.source` (Track S3): the signed URL with no credentials, no redirects and no
// referrer, and each failure as its code; over 20 MB refused before or while reading.
import { describe, expect, it } from 'vitest';
import { EmbedFailure, readSource } from './source';

const ETAG = '"v1"';
const URL_SOURCE = { kind: 'url' as const, url: 'https://blob.example/wb?sig=SECRET', etag: ETAG };

function answering(response: Response | Error) {
  const calls: { url: string; init: RequestInit | undefined }[] = [];
  const fetcher = (async (url: string, init?: RequestInit) => {
    calls.push({ url, init });
    if (response instanceof Error) throw response;
    return response;
  }) as unknown as typeof fetch;
  return { fetcher, calls };
}

async function codeOf(promise: Promise<unknown>) {
  try {
    await promise;
    return 'read';
  } catch (error) {
    expect(error).toBeInstanceOf(EmbedFailure);
    expect((error as Error).message).not.toContain('SECRET');
    return (error as EmbedFailure).code;
  }
}

/** A body that streams `chunks` of `size` bytes without saying its length. */
function streamed(chunks: number, size: number) {
  let sent = 0;
  return new Response(
    new ReadableStream({
      pull(controller) {
        if (sent++ >= chunks) controller.close();
        else controller.enqueue(new Uint8Array(size));
      },
    }),
  );
}

describe('reading the source', () => {
  it('reads a signed URL in cors mode, with no credentials, redirects or referrer', async () => {
    const { fetcher, calls } = answering(new Response(new Uint8Array([1, 2, 3])));
    const blob = await readSource(URL_SOURCE, fetcher);
    expect(blob.size).toBe(3);
    expect(calls).toHaveLength(1);
    expect(calls[0]!.url).toBe(URL_SOURCE.url);
    expect(calls[0]!.init).toMatchObject({
      method: 'GET',
      mode: 'cors',
      credentials: 'omit',
      redirect: 'error',
      referrerPolicy: 'no-referrer',
    });
  });

  it('says source.forbidden for a 403 and source.unavailable for anything else', async () => {
    expect(
      await codeOf(readSource(URL_SOURCE, answering(new Response('', { status: 403 })).fetcher)),
    ).toBe('source.forbidden');
    for (const status of [404, 500, 503]) {
      expect(
        await codeOf(readSource(URL_SOURCE, answering(new Response('', { status })).fetcher)),
      ).toBe('source.unavailable');
    }
    expect(
      await codeOf(readSource(URL_SOURCE, answering(new TypeError('Failed to fetch')).fetcher)),
    ).toBe('source.unavailable');
  });

  it('refuses over 20 MB by its length, or while it streams', async () => {
    const declared = new Response('x', {
      headers: { 'Content-Length': String(20 * 1024 * 1024 + 1) },
    });
    expect(await codeOf(readSource(URL_SOURCE, answering(declared).fetcher))).toBe(
      'workbook.tooLarge',
    );
    expect(await codeOf(readSource(URL_SOURCE, answering(streamed(21, 1024 * 1024)).fetcher))).toBe(
      'workbook.tooLarge',
    );
    expect((await readSource(URL_SOURCE, answering(streamed(20, 1024 * 1024)).fetcher)).size).toBe(
      20 * 1024 * 1024,
    );
  });

  it('takes transferred bytes up to 20 MB', async () => {
    const bytes = new ArrayBuffer(20 * 1024 * 1024);
    expect((await readSource({ kind: 'bytes', bytes, etag: ETAG })).size).toBe(bytes.byteLength);
    expect(
      await codeOf(
        readSource({ kind: 'bytes', bytes: new ArrayBuffer(20 * 1024 * 1024 + 1), etag: ETAG }),
      ),
    ).toBe('workbook.tooLarge');
  });
});
