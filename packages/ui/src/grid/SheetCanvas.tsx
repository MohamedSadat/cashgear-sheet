import { useEffect, useRef } from 'react';
import type { EngineClient, TilePalette } from '@cgs/engine-client';
import { GridController } from './controller';

interface SheetCanvasProps {
  client: EngineClient;
  palette: TilePalette;
  onError: (error: unknown) => void;
}

/** The grid's canvas. All its drawing happens in the controller, outside React. */
export function SheetCanvas({ client, palette, onError }: SheetCanvasProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const controllerRef = useRef<GridController | null>(null);
  const latestPalette = useRef(palette);

  useEffect(() => {
    const canvas = canvasRef.current!;
    const controller = new GridController(canvas, client, latestPalette.current, onError);
    controllerRef.current = controller;
    const observer = new ResizeObserver(() => {
      const { width, height } = canvas.getBoundingClientRect();
      controller.resize(width, height, window.devicePixelRatio);
    });
    observer.observe(canvas);
    return () => {
      observer.disconnect();
      controller.dispose();
      controllerRef.current = null;
    };
  }, [client, onError]);

  useEffect(() => {
    if (latestPalette.current === palette) return;
    latestPalette.current = palette;
    controllerRef.current?.setPalette(palette);
  }, [palette]);

  return <canvas ref={canvasRef} className="cgs-sheet-canvas" data-testid="sheet-canvas" />;
}
