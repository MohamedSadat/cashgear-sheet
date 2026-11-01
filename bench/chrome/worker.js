// One workbook in one fresh module, so the module's memory size is that run's peak.
self.onmessage = async ({ data }) => {
  const { engine, url, edits, topRowInsert, examples, skipCalc } = data;
  const module = await import(`/wasm/${engine}/cgs_bench.js`);
  const instance = await module.default();
  const bytes = new Uint8Array(await (await fetch(url)).arrayBuffer());
  try {
    const json = module.run_workbook(engine, bytes, edits, topRowInsert, examples, skipCalc);
    const report = JSON.parse(json);
    report.wasmMemoryBytes = instance.memory.buffer.byteLength;
    self.postMessage(report);
  } catch (error) {
    self.postMessage({
      ok: false,
      error: 'trap: ' + (error.message ?? error),
      wasmMemoryBytes: instance.memory.buffer.byteLength,
    });
  }
};
