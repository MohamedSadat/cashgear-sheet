// Conditional formats' icons, drawn as shapes on any 2D context. The numbers follow `ICON_SETS`
// in crates/cgs-engine/src/view_block.rs: a set's number times 8, plus the icon's place in it.

type Context = Pick<
  CanvasRenderingContext2D,
  | 'save'
  | 'restore'
  | 'translate'
  | 'rotate'
  | 'beginPath'
  | 'moveTo'
  | 'lineTo'
  | 'closePath'
  | 'fill'
  | 'stroke'
  | 'arc'
  | 'rect'
  | 'clip'
  | 'fillRect'
> & {
  fillStyle: string | CanvasGradient | CanvasPattern;
  strokeStyle: string | CanvasGradient | CanvasPattern;
  lineWidth: number;
  globalAlpha: number;
};

export const ICON_SETS = [
  '3Arrows',
  '3ArrowsGray',
  '3Flags',
  '3TrafficLights1',
  '3TrafficLights2',
  '3Signs',
  '3Symbols',
  '3Symbols2',
  '4Arrows',
  '4ArrowsGray',
  '4RedToBlack',
  '4Rating',
  '4TrafficLights',
  '5Arrows',
  '5ArrowsGray',
  '5Rating',
  '5Quarters',
  '3Stars',
  '3Triangles',
  '5Boxes',
];

const RED = '#e8423c';
const YELLOW = '#f5c02b';
const GREEN = '#32a852';
const GRAY = '#8c8c8c';
const BLACK = '#262626';
const BLUE = '#3a6fc4';

/** A set's icons from lowest to highest, as a shape and a colour each. */
export function iconLook(set: string, place: number): { shape: string; color: string } {
  const pick = <T>(list: T[]) => list[Math.min(place, list.length - 1)]!;
  const ramp = (count: number) => {
    const share = count <= 1 ? 1 : place / (count - 1);
    return share < 0.34 ? RED : share < 0.67 ? YELLOW : GREEN;
  };
  switch (set) {
    case '3Arrows':
      return { shape: pick(['down', 'right', 'up']), color: pick([RED, YELLOW, GREEN]) };
    case '3ArrowsGray':
      return { shape: pick(['down', 'right', 'up']), color: GRAY };
    case '4Arrows':
      return { shape: pick(['down', 'downRight', 'upRight', 'up']), color: ramp(4) };
    case '4ArrowsGray':
      return { shape: pick(['down', 'downRight', 'upRight', 'up']), color: GRAY };
    case '5Arrows':
      return { shape: pick(['down', 'downRight', 'right', 'upRight', 'up']), color: ramp(5) };
    case '5ArrowsGray':
      return { shape: pick(['down', 'downRight', 'right', 'upRight', 'up']), color: GRAY };
    case '3Flags':
      return { shape: 'flag', color: pick([RED, YELLOW, GREEN]) };
    case '3Signs':
      return { shape: pick(['diamond', 'triangle', 'circle']), color: pick([RED, YELLOW, GREEN]) };
    case '3Symbols':
      return { shape: pick(['cross', 'bang', 'tick']), color: pick([RED, YELLOW, GREEN]) };
    case '3Symbols2':
      return {
        shape: pick(['crossBare', 'bangBare', 'tickBare']),
        color: pick([RED, YELLOW, GREEN]),
      };
    case '3Stars':
      return { shape: pick(['starEmpty', 'starHalf', 'star']), color: YELLOW };
    case '3Triangles':
      return { shape: pick(['down', 'dash', 'up']), color: pick([RED, YELLOW, GREEN]) };
    case '4RedToBlack':
      return { shape: 'circle', color: pick([BLACK, GRAY, '#e69696', RED]) };
    case '4Rating':
      return { shape: 'bars' + (place + 1) + 'of4', color: BLUE };
    case '5Rating':
      return { shape: 'bars' + place + 'of4', color: BLUE };
    case '5Quarters':
      return { shape: 'quarters' + place, color: BLACK };
    case '5Boxes':
      return { shape: 'boxes' + place, color: BLUE };
    case '4TrafficLights':
      return { shape: 'circle', color: pick([BLACK, RED, YELLOW, GREEN]) };
    default:
      return { shape: 'circle', color: pick([RED, YELLOW, GREEN]) };
  }
}

/** Draws one icon in a square of `size` with its top left at `x`, `y`. */
export function drawIcon(c: Context, code: number, x: number, y: number, size: number) {
  const set = ICON_SETS[code >> 3] ?? '3TrafficLights1';
  const { shape, color } = iconLook(set, code & 7);
  const m = size / 2;
  c.save();
  c.translate(x, y);
  c.fillStyle = color;
  c.strokeStyle = color;
  c.lineWidth = Math.max(1, size / 8);
  const arrow = (angle: number) => {
    c.save();
    c.translate(m, m);
    c.rotate(angle);
    c.beginPath();
    c.moveTo(0, -m * 0.9);
    c.lineTo(m * 0.8, 0);
    c.lineTo(m * 0.3, 0);
    c.lineTo(m * 0.3, m * 0.9);
    c.lineTo(-m * 0.3, m * 0.9);
    c.lineTo(-m * 0.3, 0);
    c.lineTo(-m * 0.8, 0);
    c.closePath();
    c.fill();
    c.restore();
  };
  const star = (filled: number) => {
    const path = () => {
      c.beginPath();
      for (let at = 0; at < 10; at++) {
        const r = at % 2 ? m * 0.4 : m * 0.95;
        const a = (Math.PI / 5) * at - Math.PI / 2;
        c.lineTo(m + r * Math.cos(a), m + r * Math.sin(a));
      }
      c.closePath();
    };
    path();
    c.stroke();
    if (filled > 0) {
      c.save();
      c.beginPath();
      c.rect(0, 0, size * filled, size);
      c.clip();
      path();
      c.fill();
      c.restore();
    }
  };
  const arrows: Record<string, number> = {
    up: 0,
    down: Math.PI,
    right: Math.PI / 2,
    upRight: Math.PI / 4,
    downRight: (3 * Math.PI) / 4,
  };
  if (shape in arrows) arrow(arrows[shape]!);
  else if (shape === 'dash') c.fillRect(size * 0.2, m - size * 0.08, size * 0.6, size * 0.16);
  else if (shape.startsWith('star')) star(shape === 'star' ? 1 : shape === 'starHalf' ? 0.5 : 0);
  else if (shape === 'flag') {
    c.fillRect(size * 0.2, size * 0.1, size * 0.08, size * 0.8);
    c.beginPath();
    c.moveTo(size * 0.28, size * 0.1);
    c.lineTo(size * 0.85, size * 0.3);
    c.lineTo(size * 0.28, size * 0.5);
    c.fill();
  } else if (shape === 'diamond' || shape === 'triangle') {
    c.beginPath();
    if (shape === 'diamond') {
      c.moveTo(m, 0);
      c.lineTo(size, m);
      c.lineTo(m, size);
      c.lineTo(0, m);
    } else {
      c.moveTo(m, size * 0.05);
      c.lineTo(size * 0.95, size * 0.9);
      c.lineTo(size * 0.05, size * 0.9);
    }
    c.fill();
  } else if (shape.startsWith('bars')) {
    const filled = Number(shape[4]);
    const count = 4;
    const w = size / (count * 1.5);
    for (let at = 0; at < count; at++) {
      const h = (size * (at + 1)) / count;
      c.globalAlpha = at < filled ? 1 : 0.25;
      c.fillRect(at * w * 1.5, size - h, w, h);
    }
  } else if (shape.startsWith('quarters')) {
    const quarters = Number(shape.slice(8));
    c.beginPath();
    c.arc(m, m, m * 0.9, 0, Math.PI * 2);
    c.stroke();
    if (quarters > 0) {
      c.beginPath();
      c.moveTo(m, m);
      c.arc(m, m, m * 0.9, -Math.PI / 2, -Math.PI / 2 + (Math.PI / 2) * quarters);
      c.closePath();
      c.fill();
    }
  } else if (shape.startsWith('boxes')) {
    const filled = Number(shape.slice(5));
    const w = size / 2.4;
    for (let at = 0; at < 4; at++) {
      c.globalAlpha = at < filled ? 1 : 0.25;
      c.fillRect((at % 2) * w * 1.2, Math.floor(at / 2) * w * 1.2, w, w);
    }
  } else {
    // A filled circle, with a mark in white for the symbol sets, or the mark alone.
    if (!shape.endsWith('Bare')) {
      c.beginPath();
      c.arc(m, m, m * 0.9, 0, Math.PI * 2);
      c.fill();
    }
    c.strokeStyle = shape.endsWith('Bare') ? color : '#ffffff';
    c.lineWidth = Math.max(1, size / 7);
    c.beginPath();
    if (shape.startsWith('tick')) {
      c.moveTo(size * 0.28, m);
      c.lineTo(size * 0.45, size * 0.68);
      c.lineTo(size * 0.72, size * 0.32);
    } else if (shape.startsWith('cross')) {
      c.moveTo(size * 0.32, size * 0.32);
      c.lineTo(size * 0.68, size * 0.68);
      c.moveTo(size * 0.68, size * 0.32);
      c.lineTo(size * 0.32, size * 0.68);
    } else if (shape.startsWith('bang')) {
      c.moveTo(m, size * 0.25);
      c.lineTo(m, size * 0.58);
      c.moveTo(m, size * 0.7);
      c.lineTo(m, size * 0.76);
    }
    c.stroke();
  }
  c.restore();
}
