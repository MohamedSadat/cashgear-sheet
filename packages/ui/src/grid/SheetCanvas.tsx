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
