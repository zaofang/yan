// Runs yan.wasm off the page's thread, so a looping program can be stopped by ending the worker.
const enc = new TextEncoder();
const dec = new TextDecoder();
const zero = () => 0;
// C calls core-math makes: wasm has round-to-nearest only and no exception flags.
const env = {
  fegetround: zero, feraiseexcept: zero, feclearexcept: zero, fetestexcept: zero,
  fegetexceptflag: zero, fesetexceptflag: zero, feholdexcept: zero, feupdateenv: zero,
  puts: zero, printf: zero, nan: () => NaN,
  exit: (code) => { throw new Error(`exit(${code})`); },
};

// Some hosts serve .wasm without application/wasm, which streaming compilation refuses.
const ready = WebAssembly.instantiateStreaming(fetch("yan.wasm"), { env })
  .catch(() => fetch("yan.wasm").then((r) => r.arrayBuffer()).then((b) => WebAssembly.instantiate(b, { env })))
  .then(({ instance }) => instance.exports);

function call(x, op, head, src) {
  const bytes = enc.encode(`${head}\n${src}`);
  const ptr = x.alloc(bytes.length);
  new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
  x[op](ptr, bytes.length);
  return JSON.parse(dec.decode(new Uint8Array(x.memory.buffer, x.reply_ptr(), x.reply_len())));
}

onmessage = async ({ data: { id, op, head, src } }) => {
  try {
    postMessage({ id, ok: call(await ready, op, head, src) });
  } catch (e) {
    postMessage({ id, trap: String(e) });
  }
};
