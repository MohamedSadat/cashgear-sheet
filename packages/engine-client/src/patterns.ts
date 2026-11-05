// Excel's cell patterns as small repeating pictures. The numbers are `Hatch` in
// crates/cgs-model/src/style.rs. Rows read as strings, with # where the pattern's colour goes.

const PATTERNS: Record<number, string[]> = {
  // gray125
  1: ['..#.', '....', '#...', '....'],
  // gray0625
  2: [
    '#.......',
    '........',
    '........',
    '........',
    '....#...',
    '........',
    '........',
    '........',
  ],
  // lightGray
  3: ['..#.', '#...'],
  // mediumGray
  4: ['#.', '.#'],
  // darkGray
  5: ['#.##', '###.'],
  // darkHorizontal
  6: ['#', '#', '.', '.'],
  // darkVertical
  7: ['##..'],
  // darkDown
  8: ['##..', '.##.', '..##', '#..#'],
  // darkUp
  9: ['..##', '.##.', '##..', '#..#'],
  // darkGrid
  10: ['##..', '##..', '..##', '..##'],
  // darkTrellis
  11: ['####', '.##.', '####', '#..#'],
  // lightHorizontal
  12: ['#', '.', '.', '.'],
  // lightVertical
  13: ['#...'],
  // lightDown
  14: ['#...', '.#..', '..#.', '...#'],
  // lightUp
  15: ['...#', '..#.', '.#..', '#...'],
  // lightGrid
  16: ['####', '..#.', '..#.', '..#.'],
  // lightTrellis
  17: ['..#.', '.#.#', '#...', '.#.#'],
};

export function patternRows(pattern: number): string[] | undefined {
  return PATTERNS[pattern];
}

const made = new Map<string, OffscreenCanvas | null>();

/** The picture for a pattern in a colour, or nothing where it can't be made. */
export function patternImage(
  pattern: number,
  color: string,
  pixel: number,
): OffscreenCanvas | null {
  const key = pattern + ' ' + color + ' ' + pixel;
  const known = made.get(key);
  if (known !== undefined) return known;
  const rows = PATTERNS[pattern];
  let image: OffscreenCanvas | null = null;
  if (rows && typeof OffscreenCanvas !== 'undefined') {
    image = new OffscreenCanvas(rows[0]!.length * pixel, rows.length * pixel);
    const context = image.getContext('2d');
    if (context) {
      context.fillStyle = color;
      rows.forEach((row, y) => {
        for (let x = 0; x < row.length; x++) {
          if (row[x] === '#') context.fillRect(x * pixel, y * pixel, pixel, pixel);
        }
      });
    } else {
      image = null;
    }
  }
  if (made.size > 256) made.clear();
  made.set(key, image);
  return image;
}
