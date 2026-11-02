// Fitting a value to a column once its width is known, as only the tile worker can measure
// text. Pure: `fits` does the measuring.

/**
 * A General number shortened to fit, as Excel does when a column narrows: fewer decimals
 * first, then scientific notation with fewer digits, and `null` if nothing fits.
 */
export function shortenGeneral(text: string, fits: (candidate: string) => boolean): string | null {
  if (fits(text)) return text;
  const decimal = text.includes(',') && !text.includes('.') ? ',' : '.';
  const value = Number(decimal === ',' ? text.replace(',', '.') : text);
  if (!Number.isFinite(value)) return null;
  const local = (candidate: string) => (decimal === ',' ? candidate.replace('.', ',') : candidate);
  const scientific = /E/i.test(text);
  if (!scientific) {
    const point = text.indexOf(decimal);
    const decimals = point < 0 ? 0 : text.length - point - 1;
    for (let keep = decimals - 1; keep >= 0; keep--) {
      const candidate = local(trimZeros(value.toFixed(keep)));
      if (fits(candidate)) {
        // A value that rounds to nothing isn't worth showing as a fit.
        if (Number(candidate.replace(',', '.')) !== 0 || value === 0) return candidate;
        break;
      }
    }
  }
  for (let digits = 5; digits >= 0; digits--) {
    const candidate = local(exponent(value, digits));
    if (fits(candidate)) return candidate;
  }
  return null;
}

function trimZeros(text: string) {
  return text.includes('.') ? text.replace(/\.?0+$/, '') : text;
}

/** `1.23E+11`: Excel's form, with at least two exponent digits and no trailing zeros. */
function exponent(value: number, digits: number) {
  const [mantissa, power] = value.toExponential(digits).split('e') as [string, string];
  const sign = power.startsWith('-') ? '-' : '+';
  const magnitude = power.replace(/^[+-]/, '').padStart(2, '0');
  return trimZeros(mantissa) + 'E' + sign + magnitude;
}

/** Text broken into lines at spaces and line breaks, words split only if they must be. */
export function wrapLines(text: string, fits: (candidate: string) => boolean): string[] {
  const lines: string[] = [];
  for (const paragraph of text.split('\n')) {
    let line = '';
    for (const word of paragraph.split(/(?<= )/)) {
      const joined = line + word;
      if (fits(joined.trimEnd())) {
        line = joined;
        continue;
      }
      if (line) lines.push(line.trimEnd());
      if (fits(word.trimEnd())) {
        line = word;
        continue;
      }
      // A word wider than the cell breaks wherever it has to.
      let piece = '';
      for (const ch of word) {
        if (piece && !fits(piece + ch)) {
          lines.push(piece);
          piece = '';
        }
        piece += ch;
      }
      line = piece;
    }
    lines.push(line.trimEnd());
  }
  return lines;
}

/** Controls that a single-line cell doesn't show, line breaks included. */
export function singleLine(text: string) {
  let out = text;
  for (let at = out.length - 1; at >= 0; at--) {
    const code = out.charCodeAt(at);
    if (code < 0x20 || code === 0x7f) out = out.slice(0, at) + out.slice(at + 1);
  }
  return out;
}
