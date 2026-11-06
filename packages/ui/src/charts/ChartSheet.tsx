// A chart sheet: its one chart, drawn to fill the tab as Excel draws it, and chosen for Chart
// Design and Format as soon as the sheet shows.
import { useEffect, useRef, useState } from 'react';
import type { ChartScreen, ChartView, DrawingBox, EngineClient } from '@cgs/engine-client';
import { DrawingContent } from './DrawingLayer';
import type { EngineElement } from './chartProps';

interface ChartSheetProps {
  engine: EngineClient;
  sheet: number;
  version: number;
  screen: ChartScreen;
  element: EngineElement | null;
  onReady: (drawings: DrawingBox[]) => void;
  onView: (index: number, view: ChartView | null) => void;
  onElement: (element: EngineElement) => void;
}

export function ChartSheet(props: ChartSheetProps) {
  const { engine, sheet, version, screen } = props;
  const latest = useRef(props);
  latest.current = props;
  const host = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });
  const [box, setBox] = useState<DrawingBox | null>(null);
  const [view, setView] = useState<ChartView | null | undefined>(undefined);

  useEffect(() => {
    const element = host.current;
    if (!element) return;
    const observer = new ResizeObserver(() => {
      const { width, height } = element.getBoundingClientRect();
      setSize({ width: Math.floor(width), height: Math.floor(height) });
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    let gone = false;
    void engine.drawings(sheet).then(async (drawings) => {
      if (gone) return;
      latest.current.onReady(drawings);
      const first = drawings[0] ?? null;
      setBox(first);
      const next = first ? await engine.chart(sheet, first.index, screen) : null;
      if (gone) return;
      setView(next);
      if (first) latest.current.onView(first.index, next);
    });
    return () => {
      gone = true;
    };
  }, [engine, sheet, version, screen]);

  // Excel fits a chart sheet's chart to the window, keeping a margin.
  const width = Math.max(120, size.width - 32);
  const height = Math.max(80, size.height - 32);
  return (
    <div className="cgs-chart-sheet" ref={host} data-testid="chart-sheet">
      {box && size.width > 0 && (
        <div className="cgs-chart-sheet-frame" style={{ width, height }} data-testid="chart">
          <DrawingContent
            engine={engine}
            box={{ ...box, width, height }}
            view={view}
            selected
            element={props.element}
            onElement={(chosen) => latest.current.onElement(chosen)}
          />
        </div>
      )}
      {!box && <div className="cgs-sheet-note">This chart sheet has no chart.</div>}
    </div>
  );
}
