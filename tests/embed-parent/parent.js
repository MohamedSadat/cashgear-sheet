// Workspace's side of `sheet-embed.1`, as its docs/INTEGRATIONS.md writes it, for the browser
// suite: a frame with `sandbox="allow-scripts allow-same-origin"`, no referrer and no permissions;
// `ready` taken only from the frame's window and the sheet's origin, once per load; `init` posted
// to the sheet's origin with `port2` (and the bytes) transferred; everything after on the port.
// What arrives is kept for the test, with any bytes as base64.

(function () {
  const state = {
    sheet: '',
    frame: null,
    loads: 0,
    /** `ready` messages from the frame, with their origin. */
    ready: [],
    /** Window messages from anywhere else. */
    stray: [],
    /** Everything on the port, in order. */
    messages: [],
    /** Answers on ports of later inits, which must get none. */
    spare: [],
    port: null,
  };

  const toBase64 = (buffer) => {
    const bytes = new Uint8Array(buffer);
    let text = '';
    for (let at = 0; at < bytes.length; at += 0x8000) {
      text += String.fromCharCode(...bytes.subarray(at, at + 0x8000));
    }
    return btoa(text);
  };
  const fromBase64 = (text) => {
    const binary = atob(text);
    const bytes = new Uint8Array(binary.length);
    for (let at = 0; at < binary.length; at++) bytes[at] = binary.charCodeAt(at);
    return bytes.buffer;
  };

  /** A message as the test reads it: an `ArrayBuffer` becomes `{ $bytes: base64 }`. */
  function kept(data) {
    if (data instanceof ArrayBuffer) return { $bytes: toBase64(data) };
    if (Array.isArray(data)) return data.map(kept);
    if (data && typeof data === 'object') {
      const out = {};
      for (const [key, value] of Object.entries(data)) out[key] = kept(value);
      return out;
    }
    return data;
  }

  /** The other way: `{ $bytes: base64 }` becomes an `ArrayBuffer`, to transfer. */
  function made(data, transfer) {
    if (data && typeof data === 'object' && typeof data.$bytes === 'string') {
      const buffer = fromBase64(data.$bytes);
      transfer.push(buffer);
      return buffer;
    }
    if (data && typeof data === 'object' && typeof data.$zeros === 'number') {
      const buffer = new ArrayBuffer(data.$zeros);
      transfer.push(buffer);
      return buffer;
    }
    if (Array.isArray(data)) return data.map((item) => made(item, transfer));
    if (data && typeof data === 'object') {
      const out = {};
      for (const [key, value] of Object.entries(data)) out[key] = made(value, transfer);
      return out;
    }
    return data;
  }

  window.addEventListener('message', (event) => {
    if (state.frame && event.source === state.frame.contentWindow) {
      state.ready.push({ origin: event.origin, data: kept(event.data) });
    } else {
      state.stray.push({ origin: event.origin, data: kept(event.data) });
    }
  });

  window.cgsParent = {
    state,
    /** Frames the sheet's page, as Workspace's block does. */
    open({
      sheet,
      path = '/embed',
      sandbox = 'allow-scripts allow-same-origin',
      width = 800,
      height = 600,
    }) {
      state.sheet = new URL(sheet).origin;
      const frame = document.createElement('iframe');
      frame.setAttribute('sandbox', sandbox);
      frame.setAttribute('referrerpolicy', 'no-referrer');
      frame.setAttribute('allow', '');
      frame.title = 'Workbook';
      frame.style.width = width + 'px';
      frame.style.height = height + 'px';
      frame.addEventListener('load', () => state.loads++);
      frame.src = state.sheet + path;
      document.body.appendChild(frame);
      state.frame = frame;
    },
    /** Frames another page beside it, as an iframe the embed must not listen to. */
    openOther(url) {
      const frame = document.createElement('iframe');
      frame.src = url;
      frame.id = 'other';
      document.body.appendChild(frame);
      return new Promise((done) => frame.addEventListener('load', () => done()));
    },
    /**
     * Posts `init` with a new port; `origin` is where it is aimed, the sheet's unless given. A
     * `spare` init keeps its answers apart and leaves the port in use as it is.
     */
    init(message, { origin, spare = false } = {}) {
      const channel = new MessageChannel();
      if (spare) {
        channel.port1.onmessage = (event) => state.spare.push(kept(event.data));
      } else {
        channel.port1.onmessage = (event) => state.messages.push(kept(event.data));
        state.port = channel.port1;
      }
      const transfer = [channel.port2];
      const data = made(message, transfer);
      state.frame.contentWindow.postMessage(data, origin ?? state.sheet, transfer);
    },
    /** Posts on the port. */
    send(message) {
      const transfer = [];
      const data = made(message, transfer);
      state.port.postMessage(data, transfer);
    },
    /** Posts on the window without a port, or anything else the embed must ignore. */
    post(message, ports = 0) {
      const channels = Array.from({ length: ports }, () => new MessageChannel());
      for (const channel of channels) {
        channel.port1.onmessage = (event) =>
          state.stray.push({ port: true, data: kept(event.data) });
      }
      state.frame.contentWindow.postMessage(
        message,
        state.sheet,
        channels.map((channel) => channel.port2),
      );
    },
    resize(width, height) {
      state.frame.style.width = width + 'px';
      state.frame.style.height = height + 'px';
    },
  };
})();
