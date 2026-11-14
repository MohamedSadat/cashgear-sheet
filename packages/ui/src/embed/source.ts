// Reads an embed's workbook from `init.source`, in memory only. A signed URL is read with `fetch`
// in `cors` mode, no credentials, no redirects and no referrer (Workspace's contract); bytes come
// transferred. Over 20 MB is refused before or while reading, so a big file never sits in memory.
import { MAX_WORKBOOK_BYTES, type ErrorCode, type Source } from './contract';

/** Why a source couldn't be read, by the contract's code. Its message names no URL. */
export class EmbedFailure extends Error {
  constructor(
    readonly code: ErrorCode,
    message: string,
  ) {
    super(message);
  }
}

const XLSX = 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet';

/** The workbook's bytes as a blob, or an `EmbedFailure`. */
export async function readSource(
  source: Source,
  fetcher: typeof fetch = fetch,
  limit = MAX_WORKBOOK_BYTES,
): Promise<Blob> {
  if (source.kind === 'bytes') {
    if (source.bytes.byteLength > limit) {
      throw new EmbedFailure('workbook.tooLarge', 'The workbook is over 20 MB.');
    }
    return new Blob([source.bytes], { type: XLSX });
  }
  let response: Response;
  try {
    response = await fetcher(source.url, {
      method: 'GET',
      mode: 'cors',
      credentials: 'omit',
      redirect: 'error',
      referrerPolicy: 'no-referrer',
      cache: 'no-store',
    });
  } catch {
    throw new EmbedFailure('source.unavailable', 'The workbook couldn’t be read.');
  }
  if (response.status === 403) {
    void response.body?.cancel().catch(() => undefined);
    throw new EmbedFailure('source.forbidden', 'Blob Storage refused the URL.');
  }
  if (!response.ok) {
    void response.body?.cancel().catch(() => undefined);
    throw new EmbedFailure(
      'source.unavailable',
      `The workbook couldn’t be read (${response.status}).`,
    );
  }
  const declared = Number(response.headers.get('Content-Length'));
  if (Number.isFinite(declared) && declared > limit) {
    void response.body?.cancel().catch(() => undefined);
    throw new EmbedFailure('workbook.tooLarge', 'The workbook is over 20 MB.');
  }
  if (!response.body) return new Blob([await response.arrayBuffer()], { type: XLSX });
  const reader = response.body.getReader();
  const chunks: Uint8Array[] = [];
  let total = 0;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      total += value.byteLength;
      if (total > limit) {
        await reader.cancel().catch(() => undefined);
        throw new EmbedFailure('workbook.tooLarge', 'The workbook is over 20 MB.');
      }
      chunks.push(value);
    }
  } catch (error) {
    if (error instanceof EmbedFailure) throw error;
    throw new EmbedFailure('source.unavailable', 'The workbook couldn’t be read.');
  }
  return new Blob(chunks as BlobPart[], { type: XLSX });
}
