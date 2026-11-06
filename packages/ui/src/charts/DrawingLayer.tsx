// Charts and pictures over the grid. The controller says after each frame where each one shows
// in each pane, and this puts it there, clipped to the pane, without React drawing again: React
// only draws when a drawing first shows in a pane or what it shows changes. A drawing is chosen
// by a click, moved by dragging it and sized by its handles; the engine gets one command when the
// pointer lets go.
import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from 'react';
import { CgChart, type CgChartElementPointerDetail } from '@cashgear/ui';
import type { ChartScreen, ChartView, DrawingBox, EngineClient } from '@cgs/engine-client';
import { drawnByLayer, type DrawingFrame, type GridController } from '../grid/controller';
import { chartElement, chartProps, engineElement, type EngineElement } from './chartProps';

/** A drawing's place and size in sheet pixels at 100%. */
export interface DrawingPlace {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface DrawingLayerProps {
  engine: EngineClient;
  controller: GridController | null;
  sheet: number;
  drawings: DrawingBox[];
  /** Bumped whenever the workbook changes, so charts read their cells again. */
  version: number;
  screen: ChartScreen;
  selected: number | null;
  element: EngineElement | null;
  onSelect: (index: number | null, element: EngineElement | null) => void;
  onPlace: (index: number, place: DrawingPlace) => void;
  onDelete: (index: number) => void;
  onFormat: (index: number) => void;
  onContextMenu: (index: number, x: number, y: number) => void;
  /** Each chart's view as it is read, for the ribbon. */
  onView: (index: number, view: ChartView | null) => void;
  /** Keyboard focus goes back to the grid. */
  onLeave: () => void;
}

const PANES = [0, 1, 2, 3];
const HANDLES = ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w'] as const;
type Handle = (typeof HANDLES)[number];

/** The outline a drag shows, until the engine has the drawing's new place. */
interface Drag {
  index: number;
  handle: Handle | null;
  startX: number;
  startY: number;
  start: DrawingPlace;
  zoom: number;
  moved: boolean;
  keepRatio: boolean;
}

/** Where a drag leaves a drawing. Corners keep a picture's proportions with Shift, as in Excel. */
export function dragged(drag: Drag, dx: number, dy: number, shift: boolean): DrawingPlace {
  const { start, handle } = drag;
  const zx = dx / drag.zoom;
  const zy = dy / drag.zoom;
  if (!handle) return { ...start, x: Math.round(start.x + zx), y: Math.round(start.y + zy) };
  let { x, y, width, height } = start;
  if (handle.includes('e')) width = start.width + zx;
  if (handle.includes('s')) height = start.height + zy;
  if (handle.includes('w')) {
    width = start.width - zx;
    x = start.x + zx;
  }
  if (handle.includes('n')) {
    height = start.height - zy;
    y = start.y + zy;
  }
  const corner = handle.length === 2;
  if (corner && (shift || drag.keepRatio) && start.height > 0) {
    const ratio = start.width / start.height;
    if (Math.abs(width / ratio) > Math.abs(height)) height = width / ratio;
    else width = height * ratio;
    if (handle.includes('w')) x = start.x + start.width - width;
    if (handle.includes('n')) y = start.y + start.height - height;
  }
  // At least a few pixels, as Excel keeps.
  width = Math.max(8, width);
  height = Math.max(8, height);
  return {
    x: Math.round(Math.max(0, x)),
    y: Math.round(Math.max(0, y)),
    width: Math.round(width),
    height: Math.round(height),
  };
}

/** A picture's bytes as an address the page can show, made once per part. */
function usePictureUrl(engine: EngineClient, part: string | undefined) {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    if (!part) return;
    let gone = false;
    let made: string | null = null;
    engine
      .media(part)
      .then((bytes) => {
        if (gone) return;
        const type = part.endsWith('.png')
          ? 'image/png'
          : part.endsWith('.gif')
            ? 'image/gif'
            : part.endsWith('.bmp')
              ? 'image/bmp'
              : part.endsWith('.svg')
                ? 'image/svg+xml'
                : 'image/jpeg';
        made = URL.createObjectURL(new Blob([bytes as BlobPart], { type }));
        setUrl(made);
      })
      .catch(() => setUrl(null));
    return () => {
      gone = true;
      if (made) URL.revokeObjectURL(made);
    };
  }, [engine, part]);
  return url;
}

const KIND_NAMES: Record<string, string> = {
  waterfall: 'Waterfall',
  funnel: 'Funnel',
  boxWhisker: 'Box and Whisker',
  clusteredColumn: 'Histogram',
  paretoLine: 'Pareto',
  sunburst: 'Sunburst',
  treemap: 'Treemap',
  regionMap: 'Map',
};

interface ContentProps {
  engine: EngineClient;
  box: DrawingBox;
  view: ChartView | null | undefined;
  selected: boolean;
  element: EngineElement | null;
  onElement: (element: EngineElement) => void;
}

/** What one drawing shows: a chart, a picture, or a placeholder naming it. */
const DrawingContent = memo(function DrawingContent({
  engine,
  box,
  view,
  selected,
  element,
  onElement,
}: ContentProps) {
  const picture = usePictureUrl(engine, box.kind === 'picture' ? box.image : undefined);
  const props = useMemo(
    () => (view && view.kind === 'classic' ? chartProps(view, box.width, box.height) : null),
    [view, box.width, box.height],
  );
  const highlighted = useMemo(
    () => (view && selected ? chartElement(view, element) : null),
    [view, selected, element],
  );
  const onPointer = useCallback(
    (detail: CgChartElementPointerDetail) => {
      if (view) onElement(engineElement(view, detail.element));
    },
    [view, onElement],
  );
  if (box.kind === 'picture') {
    return picture ? (
      <img
        className="cgs-drawing-picture"
        src={picture}
        alt={box.description ?? box.name}
        draggable={false}
      />
    ) : (
      <div className="cgs-drawing-placeholder">{box.name}</div>
    );
  }
  if (!view) return <div className="cgs-drawing-placeholder" />;
  if (!props) {
    const kind = view.extended ? (KIND_NAMES[view.extended] ?? view.extended) : null;
    return (
      <div className="cgs-drawing-placeholder" data-testid="chart-placeholder">
        {kind
          ? `${kind} chart. This chart type is kept as it is, and shows in Excel 2016 or later.`
          : 'This chart couldn’t be read. It is kept as it is.'}
      </div>
    );
  }
  return (
    <CgChart
      {...props}
      className="cgs-drawing-chart"
      ariaLabel={view.title?.text || box.name}
      onElementPointerDown={selected ? onPointer : undefined}
      highlightedElement={highlighted}
    />
  );
});

export function DrawingLayer(props: DrawingLayerProps) {
  const latest = useRef(props);
  latest.current = props;
  const { engine, controller, sheet, drawings, version, screen, selected, element } = props;
  const [views, setViews] = useState<Map<number, ChartView | null>>(new Map());
  // Which drawings show in which panes, as `index:pane`. A drawing stays once it has shown.
  const [mounted, setMounted] = useState<Set<string>>(new Set());
  const mountedRef = useRef(mounted);
  mountedRef.current = mounted;
  const panes = useRef(new Map<number, HTMLDivElement>());
  const hosts = useRef(new Map<string, HTMLDivElement>());
  const lastFrames = useRef<DrawingFrame[]>([]);
  const outline = useRef<HTMLDivElement>(null);
  const drag = useRef<Drag | null>(null);
  const [dragPlace, setDragPlace] = useState<{ index: number; place: DrawingPlace } | null>(null);

  const shown = useMemo(() => drawings.filter(drawnByLayer), [drawings]);
  const byIndex = useMemo(() => new Map(shown.map((box) => [box.index, box])), [shown]);

  // Charts read their cells again whenever something changed.
  useEffect(() => {
    let gone = false;
    const charts = shown.filter((box) => box.kind === 'chart');
    void Promise.all(
      charts.map((box) =>
        engine
          .chart(sheet, box.index, screen)
          .then((view) => [box.index, view] as const)
          .catch(() => [box.index, null] as const),
      ),
    ).then((list) => {
      if (gone) return;
      setViews(new Map(list));
      for (const [index, view] of list) latest.current.onView(index, view);
    });
    return () => {
      gone = true;
    };
  }, [engine, sheet, shown, version, screen]);

  // A new sheet starts with nothing shown.
  useEffect(() => {
    setMounted(new Set());
    hosts.current.clear();
  }, [sheet]);

  const place = useCallback((frames: DrawingFrame[]) => {
    lastFrames.current = frames;
    const seen = new Set<string>();
    const wanted: string[] = [];
    const clips = new Map<number, DrawingFrame['clip']>();
    for (const frame of frames) {
      const key = frame.index + ':' + frame.pane;
      seen.add(key);
      clips.set(frame.pane, frame.clip);
      const host = hosts.current.get(key);
      if (!host) {
        if (!mountedRef.current.has(key)) wanted.push(key);
        continue;
      }
      host.style.display = '';
      host.style.transform = `translate(${frame.x - frame.clip.x}px, ${frame.y - frame.clip.y}px) scale(${frame.zoom})`;
      host.style.setProperty('--cgs-zoom', String(frame.zoom));
    }
    for (const [pane, element] of panes.current) {
      const clip = clips.get(pane);
      if (!clip) {
        element.style.display = 'none';
        continue;
      }
      element.style.display = '';
      element.style.left = clip.x + 'px';
      element.style.top = clip.y + 'px';
      element.style.width = clip.width + 'px';
      element.style.height = clip.height + 'px';
    }
    for (const [key, host] of hosts.current) {
      if (!seen.has(key)) host.style.display = 'none';
    }
    if (wanted.length) {
      setMounted((before) => {
        const next = new Set(before);
        for (const key of wanted) next.add(key);
        return next;
      });
    }
    // The drag outline follows the drawing's frame in the scrolling pane.
    const box = outline.current;
    const active = drag.current;
    if (box && active?.moved) {
      const frame = frames.find((f) => f.index === active.index) ?? null;
      if (!frame) box.style.display = 'none';
    }
  }, []);

  useEffect(() => {
    if (!controller) return;
    controller.setDrawingListener((frameSheet, frames) => {
      if (frameSheet === latest.current.sheet) place(frames);
    });
    return () => controller.setDrawingListener(null);
  }, [controller, place]);

  // Hosts just made get their places from the last frame.
  useLayoutEffect(() => {
    place(lastFrames.current);
  }, [mounted, place, dragPlace]);

  // The selected drawing takes the keyboard.
  useEffect(() => {
    if (selected === null) return;
    const host =
      hosts.current.get(selected + ':0') ??
      [...hosts.current.entries()].find(([key]) => key.startsWith(selected + ':'))?.[1];
    if (host && document.activeElement !== host && !host.contains(document.activeElement))
      host.focus({ preventScroll: true });
  }, [selected, mounted]);

  const onElement = useCallback((index: number, chosen: EngineElement) => {
    latest.current.onSelect(index, chosen);
  }, []);

  const frameOf = (index: number) =>
    lastFrames.current.find((frame) => frame.index === index && frame.pane === 0) ??
    lastFrames.current.find((frame) => frame.index === index);

  const startDrag = (
    event: ReactPointerEvent<HTMLDivElement>,
    index: number,
    handle: Handle | null,
  ) => {
    if (event.button !== 0) return;
    const box = byIndex.get(index);
    const frame = frameOf(index);
    if (!box || !frame) return;
    if (latest.current.selected !== index)
      latest.current.onSelect(index, handle ? null : { kind: 'chartArea' });
    drag.current = {
      index,
      handle,
      startX: event.clientX,
      startY: event.clientY,
      start: { x: box.x, y: box.y, width: box.width, height: box.height },
      zoom: frame.zoom,
      moved: false,
      keepRatio: box.kind === 'picture',
    };
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    event.stopPropagation();
  };

  const moveDrag = (event: ReactPointerEvent<HTMLDivElement>) => {
    const active = drag.current;
    if (!active) return;
    const dx = event.clientX - active.startX;
    const dy = event.clientY - active.startY;
    if (!active.moved && Math.hypot(dx, dy) < 3) return;
    active.moved = true;
    setDragPlace({ index: active.index, place: dragged(active, dx, dy, event.shiftKey) });
  };

  const endDrag = (event: ReactPointerEvent<HTMLDivElement>) => {
    const active = drag.current;
    drag.current = null;
    if (!active) return;
    setDragPlace(null);
    if (!active.moved) return;
    const next = dragged(
      active,
      event.clientX - active.startX,
      event.clientY - active.startY,
      event.shiftKey,
    );
    latest.current.onPlace(active.index, next);
  };

  const onKey = (event: React.KeyboardEvent<HTMLDivElement>, index: number) => {
    if (event.key === 'Delete' || event.key === 'Backspace') {
      event.preventDefault();
      latest.current.onDelete(index);
    } else if (event.key === 'Escape') {
      event.preventDefault();
      latest.current.onSelect(null, null);
      latest.current.onLeave();
    } else if (event.key.startsWith('Arrow') && byIndex.get(index)) {
      // Excel nudges a selected object by a pixel.
      event.preventDefault();
      const box = byIndex.get(index)!;
      const step = event.ctrlKey ? 1 : 1;
      const dx = event.key === 'ArrowLeft' ? -step : event.key === 'ArrowRight' ? step : 0;
      const dy = event.key === 'ArrowUp' ? -step : event.key === 'ArrowDown' ? step : 0;
      latest.current.onPlace(index, {
        x: Math.max(0, box.x + dx),
        y: Math.max(0, box.y + dy),
        width: box.width,
        height: box.height,
      });
    }
  };

  // A wheel over a drawing scrolls the sheet.
  const onWheel = useCallback(
    (event: WheelEvent) => {
      event.preventDefault();
      controller?.wheel(event);
    },
    [controller],
  );
  const wheelTargets = useRef(new Set<HTMLDivElement>());
  const hostRef = (key: string) => (element: HTMLDivElement | null) => {
    if (element) {
      hosts.current.set(key, element);
      if (!wheelTargets.current.has(element)) {
        element.addEventListener('wheel', onWheel, { passive: false });
        wheelTargets.current.add(element);
      }
    } else {
      const old = hosts.current.get(key);
      if (old) {
        old.removeEventListener('wheel', onWheel);
        wheelTargets.current.delete(old);
      }
      hosts.current.delete(key);
    }
  };

  return (
    <div className="cgs-drawings" data-testid="drawing-layer">
      {PANES.map((pane) => (
        <div
          key={pane}
          className="cgs-drawing-pane"
          style={{ display: 'none' }}
          ref={(element) => {
            if (element) panes.current.set(pane, element);
            else panes.current.delete(pane);
          }}
        >
          {shown.map((box) => {
            const key = box.index + ':' + pane;
            if (!mounted.has(key)) return null;
            const isSelected = selected === box.index;
            const live = dragPlace?.index === box.index ? dragPlace.place : null;
            return (
              <div
                key={key}
                ref={hostRef(key)}
                className={'cgs-drawing' + (isSelected ? ' cgs-drawing-selected' : '')}
                data-testid={box.kind === 'chart' ? 'chart' : 'picture'}
                data-index={box.index}
                data-name={box.name}
                tabIndex={isSelected ? 0 : -1}
                role="figure"
                aria-label={box.name}
                style={{ width: box.width, height: box.height, display: 'none' }}
                onPointerDown={(event) => startDrag(event, box.index, null)}
                onPointerMove={moveDrag}
                onPointerUp={endDrag}
                onPointerCancel={endDrag}
                onDoubleClick={() => latest.current.onFormat(box.index)}
                onContextMenu={(event) => {
                  event.preventDefault();
                  latest.current.onSelect(
                    box.index,
                    latest.current.element ?? { kind: 'chartArea' },
                  );
                  latest.current.onContextMenu(box.index, event.clientX, event.clientY);
                }}
                onKeyDown={(event) => onKey(event, box.index)}
              >
                <DrawingContent
                  engine={engine}
                  box={box}
                  view={views.get(box.index)}
                  selected={isSelected}
                  element={isSelected ? element : null}
                  onElement={(chosen) => onElement(box.index, chosen)}
                />
                {isSelected && (
                  <div className="cgs-drawing-frame" aria-hidden="true">
                    {HANDLES.map((handle) => (
                      <div
                        key={handle}
                        className={'cgs-drawing-handle cgs-drawing-handle-' + handle}
                        data-handle={handle}
                        onPointerDown={(event) => startDrag(event, box.index, handle)}
                        onPointerMove={moveDrag}
                        onPointerUp={endDrag}
                        onPointerCancel={endDrag}
                      />
                    ))}
                  </div>
                )}
                {live && (
                  <div
                    ref={outline}
                    className="cgs-drawing-outline"
                    style={{
                      left: live.x - box.x,
                      top: live.y - box.y,
                      width: live.width,
                      height: live.height,
                    }}
                  />
                )}
              </div>
            );
          })}
        </div>
      ))}
    </div>
  );
}
