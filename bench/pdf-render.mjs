// Renders PDF pages to PNG with pdf.js in a Chrome page served by the app's dev server, for
// reviewing the app's PDFs beside Excel's. pdf.js is the copy @cashgear/ui vendors.
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { root } from '../scripts/lib.mjs';

const build = join(root, 'packages/ui/node_modules/@cashgear/ui/src/vendor/pdfjs/build')
  .split(String.fromCharCode(92))
  .join('/');

/** Writes each page as `<prefix>-<n>.png` at `scale` (1 is 72 dpi). Returns the paths. */
export async function renderPdf(page, bytes, prefix, scale = 2) {
  const shots = await page.evaluate(
    async ({ base64, build, scale }) => {
      const pdfjs = await import(`/@fs/${build}/pdf.mjs`);
      pdfjs.GlobalWorkerOptions.workerSrc = `/@fs/${build}/pdf.worker.mjs`;
      const binary = atob(base64);
      const data = new Uint8Array(binary.length);
      for (let at = 0; at < binary.length; at++) data[at] = binary.charCodeAt(at);
      const pdf = await pdfjs.getDocument({ data, isEvalSupported: false }).promise;
      const out = [];
      for (let n = 1; n <= pdf.numPages; n++) {
        const pdfPage = await pdf.getPage(n);
        const viewport = pdfPage.getViewport({ scale });
        const canvas = document.createElement('canvas');
        canvas.width = Math.ceil(viewport.width);
        canvas.height = Math.ceil(viewport.height);
        await pdfPage.render({ canvasContext: canvas.getContext('2d'), viewport }).promise;
        out.push(canvas.toDataURL('image/png').split(',')[1]);
      }
      return out;
    },
    { base64: Buffer.from(bytes).toString('base64'), build, scale },
  );
  return shots.map((shot, index) => {
    const path = `${prefix}-${index + 1}.png`;
    writeFileSync(path, Buffer.from(shot, 'base64'));
    return path;
  });
}
