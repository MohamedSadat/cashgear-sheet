// Carlito (OFL, see fonts/OFL.txt) has Calibri's character widths. It is split the way
// Google Fonts splits it, so a face only costs what a sheet's text needs.
const LATIN =
  'U+0000-00FF, U+0131, U+0152-0153, U+02BB-02BC, U+02C6, U+02DA, U+02DC, U+2000-206F, U+20AC, U+2122, U+2191, U+2193, U+2212, U+2215, U+FEFF, U+FFFD';
const LATIN_EXT =
  'U+0100-02BA, U+02BD-02C5, U+02C7-02CC, U+02CE-02D7, U+02DD-02FF, U+1D00-1DBF, U+1E00-1E9F, U+1EF2-1EFF, U+2020, U+20A0-20AB, U+20AD-20C0, U+2113, U+2C60-2C7F, U+A720-A7FF';

function face(url: URL, unicodeRange: string, weight: string, style: string) {
  return new FontFace('Carlito', 'url(' + url.href + ') format("woff2")', {
    unicodeRange,
    weight,
    style,
  });
}

export function carlitoFaces(): FontFace[] {
  return [
    face(
      new URL('./fonts/carlito-latin-400-normal.woff2', import.meta.url),
      LATIN,
      '400',
      'normal',
    ),
    face(
      new URL('./fonts/carlito-latin-400-italic.woff2', import.meta.url),
      LATIN,
      '400',
      'italic',
    ),
    face(
      new URL('./fonts/carlito-latin-700-normal.woff2', import.meta.url),
      LATIN,
      '700',
      'normal',
    ),
    face(
      new URL('./fonts/carlito-latin-700-italic.woff2', import.meta.url),
      LATIN,
      '700',
      'italic',
    ),
    face(
      new URL('./fonts/carlito-latin-ext-400-normal.woff2', import.meta.url),
      LATIN_EXT,
      '400',
      'normal',
    ),
    face(
      new URL('./fonts/carlito-latin-ext-400-italic.woff2', import.meta.url),
      LATIN_EXT,
      '400',
      'italic',
    ),
    face(
      new URL('./fonts/carlito-latin-ext-700-normal.woff2', import.meta.url),
      LATIN_EXT,
      '700',
      'normal',
    ),
    face(
      new URL('./fonts/carlito-latin-ext-700-italic.woff2', import.meta.url),
      LATIN_EXT,
      '700',
      'italic',
    ),
  ];
}
