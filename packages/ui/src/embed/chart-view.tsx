// A chart view in the embed (Track S4): the chart at its own size, as the editor draws it, in the
// theme's colours. A chart this app can't draw shows its frame and name.
import { createRoot, type Root } from 'react-dom/client';
import { flushSync } from 'react-dom';
import { CgChart } from '@cashgear/ui';
import type { ChartView, DrawingBox } from '@cgs/engine-client';
import { chartProps } from '../charts/chartProps';

export class ChartFigure {
  /** Scrolls when the chart is larger than the frame. */
  private readonly host: HTMLDivElement;
  private readonly inner: HTMLDivElement;
  private readonly root: Root;

  constructor(parent: HTMLElement) {
    this.host = document.createElement('div');
    this.host.className = 'cgs-embed-chart';
    this.host.hidden = true;
    this.inner = document.createElement('div');
    this.inner.className = 'cgs-embed-chart-box';
    this.inner.dataset.testid = 'embed-chart';
    this.host.appendChild(this.inner);
    parent.appendChild(this.host);
    this.root = createRoot(this.inner);
  }

  show(
    view: ChartView | null,
    drawing: DrawingBox,
    locale: string,
    label: string,
    unreadable: string,
  ) {
    const width = Math.max(1, Math.round(drawing.width));
    const height = Math.max(1, Math.round(drawing.height));
    this.host.hidden = false;
    this.inner.style.width = width + 'px';
    this.inner.style.height = height + 'px';
    flushSync(() =>
      this.root.render(
        view && view.kind === 'classic' ? (
          <CgChart {...chartProps(view, width, height, locale)} ariaLabel={label} />
        ) : (
          <div className="cgs-embed-unreadable" role="img" aria-label={unreadable}>
            {unreadable}
          </div>
        ),
      ),
    );
  }

  hide() {
    this.host.hidden = true;
    flushSync(() => this.root.render(null));
  }

  dispose() {
    this.root.unmount();
    this.host.remove();
  }
}
