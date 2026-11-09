import { useEffect, useRef } from 'react';
import type { EngineClient, TilePalette } from '@cgs/engine-client';
import { GridController, type GridEvents } from './controller';
import type { GridColors } from './draw';

interface SheetCanvasProps {
  client: EngineClient;
  palette: TilePalette;
  colors: GridColors;
  events: GridEvents;
  /** Hands over the controller, which the shell tells what to show. */
  onController: (controller: GridController | null) => void;
}

/** The grid's canvas. All its drawing happens in the controller, outside React. */
export function SheetCanvas({ client, palette, colors, events, onController }: SheetCanvasProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const controllerRef = useRef<GridController | null>(null);
  const latest = useRef({ palette, colors, events });
  latest.current.events = events;

  useEffect(() => {
    const canvas = canvasRef.current!;
    const proxied: GridEvents = {
      onSelection: (selection) => latest.current.events.onSelection(selection),
      onZoom: (zoom) => latest.current.events.onZoom(zoom),
      onError: (error) => latest.current.events.onError(error),
      onType: () => latest.current.events.onType?.(),
      onAction: (action) => latest.current.events.onAction?.(action),
      onJump: (rows, cols, extend) => latest.current.events.onJump?.(rows, cols, extend),
      onEditClick: (row, col) => latest.current.events.onEditClick?.(row, col) ?? false,
      onEditDrag: (row, col) => latest.current.events.onEditDrag?.(row, col),
      onDoubleCell: (row, col) => latest.current.events.onDoubleCell?.(row, col) ?? false,
      onFill: (from, to) => latest.current.events.onFill?.(from, to),
      onViewport: (sheet, range) => latest.current.events.onViewport?.(sheet, range),
      onSelectEnd: () => latest.current.events.onSelectEnd?.(),
      onResize: (axis, spans, px) => latest.current.events.onResize?.(axis, spans, px),
      onAutoFit: (axis, spans) => latest.current.events.onAutoFit?.(axis, spans),
      onContextMenu: (x, y, region) => latest.current.events.onContextMenu?.(x, y, region),
      onResizeHint: (hint) => latest.current.events.onResizeHint?.(hint),
      onCellButton: (button, box) => latest.current.events.onCellButton?.(button, box),
      onHover: (cell, x, y) => latest.current.events.onHover?.(cell, x, y),
      onCellClick: (row, col) => latest.current.events.onCellClick?.(row, col),
    };
    const controller = new GridController(
      canvas,
      client,
      latest.current.palette,
      latest.current.colors,
      proxied,
    );
    controllerRef.current = controller;
    onController(controller);
    const observer = new ResizeObserver(() => {
      const { width, height } = canvas.getBoundingClientRect();
      controller.resize(width, height, window.devicePixelRatio);
    });
    observer.observe(canvas);
    return () => {
      observer.disconnect();
      onController(null);
      controller.dispose();
      controllerRef.current = null;
    };
  }, [client, onController]);

  useEffect(() => {
    if (latest.current.palette === palette && latest.current.colors === colors) return;
    latest.current.palette = palette;
    latest.current.colors = colors;
    controllerRef.current?.setTheme(palette, colors);
  }, [palette, colors]);

  return (
    <canvas ref={canvasRef} className="cgs-sheet-canvas" data-testid="sheet-canvas" tabIndex={0} />
  );
}
