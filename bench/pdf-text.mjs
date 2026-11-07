// The text of a PDF's pages, as pdf.js reads it, in the order it reads it. pdf.js is the copy
// @cashgear/ui vendors; here it is a test tool.
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { root } from '../scripts/lib.mjs';

// pdf.js's generic build wants these at load; reading text never uses them.
class Matrix {
  constructor(init = [1, 0, 0, 1, 0, 0]) {
    [this.a, this.b, this.c, this.d, this.e, this.f] = init;
  }
  multiplySelf() {
    return this;
  }
  preMultiplySelf() {
    return this;
  }
  invertSelf() {
    return this;
  }
  translate() {
    return new Matrix();
  }
  scale() {
    return new Matrix();
  }
}
globalThis.DOMMatrix ??= Matrix;
globalThis.Path2D ??= class {};
globalThis.ImageData ??= class {};
// And newer typed array methods than this Node has.
Uint8Array.prototype.toHex ??= function toHex() {
  return Buffer.from(this.buffer, this.byteOffset, this.byteLength).toString('hex');
};
Uint8Array.prototype.toBase64 ??= function toBase64() {
  return Buffer.from(this.buffer, this.byteOffset, this.byteLength).toString('base64');
};
Math.sumPrecise ??= (values) => [...values].reduce((sum, value) => sum + value, 0);
Uint8Array.fromHex ??= (text) => new Uint8Array(Buffer.from(text, 'hex'));
Uint8Array.fromBase64 ??= (text) => new Uint8Array(Buffer.from(text, 'base64'));

const build = join(root, 'packages/ui/node_modules/@cashgear/ui/src/vendor/pdfjs/build');
const pdfjs = await import(pathToFileURL(join(build, 'pdf.mjs')).href);
pdfjs.GlobalWorkerOptions.workerSrc = pathToFileURL(join(build, 'pdf.worker.mjs')).href;

/** Each page's text: runs joined by spaces, lines by new lines. */
export async function pdfText(bytes) {
  const pdf = await pdfjs.getDocument({
    data: new Uint8Array(bytes),
    isEvalSupported: false,
    disableFontFace: true,
    useSystemFonts: false,
  }).promise;
  const pages = [];
  for (let n = 1; n <= pdf.numPages; n++) {
    const content = await (await pdf.getPage(n)).getTextContent();
    let text = '';
    for (const item of content.items) {
      text += item.str + (item.hasEOL ? '\n' : ' ');
    }
    pages.push(text.replace(/ +\n/g, '\n').trim());
  }
  await pdf.cleanup?.();
  return pages;
}
