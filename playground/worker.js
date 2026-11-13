// Runs yan.wasm off the page's thread, so a looping program can be stopped by ending the worker.
const enc = new TextEncoder();
const dec = new TextDecoder();

// The WASI calls yan.wasm imports. There is no file system; stdout and stderr go to the console
// (a program's own output comes back in the run reply, not through fd_write).
const ENOSYS = 52, EBADF = 8;
let memory;
const view = () => new DataView(memory.buffer);
const bytes = (ptr, len) => new Uint8Array(memory.buffer, ptr, len);
const wasi = {
  clock_time_get: (_id, _precision, out) => {
    const ns = BigInt(Math.round((performance.timeOrigin + performance.now()) * 1e6));
    view().setBigUint64(out, ns, true);
    return 0;
  },
  random_get: (ptr, len) => {
    for (let at = 0; at < len; at += 65536) crypto.getRandomValues(bytes(ptr + at, Math.min(65536, len - at)));
    return 0;
  },
  fd_write: (fd, iovs, count, written) => {
    const v = view();
    let text = "", n = 0;
    for (let i = 0; i < count; i++) {
      const ptr = v.getUint32(iovs + i * 8, true), len = v.getUint32(iovs + i * 8 + 4, true);
      text += dec.decode(bytes(ptr, len));
      n += len;
    }
    (fd === 2 ? console.error : console.log)(text);
    v.setUint32(written, n, true);
    return 0;
  },
  fd_read: (_fd, _iovs, _count, read) => {
    view().setUint32(read, 0, true);
    return 0;
  },
  args_sizes_get: (count, size) => {
    view().setUint32(count, 0, true);
    view().setUint32(size, 0, true);
    return 0;
  },
  args_get: () => 0,
  environ_sizes_get: (count, size) => {
    view().setUint32(count, 0, true);
    view().setUint32(size, 0, true);
    return 0;
  },
  environ_get: () => 0,
  fd_prestat_get: () => EBADF,
  sched_yield: () => 0,
  proc_exit: (code) => { throw new Error(`exit(${code})`); },
};
for (const name of ["fd_close", "fd_fdstat_get", "fd_filestat_get", "fd_prestat_dir_name", "fd_seek",
  "path_filestat_get", "path_open", "path_readlink", "poll_oneoff"]) wasi[name] = () => ENOSYS;

// Some hosts serve .wasm without application/wasm, which streaming compilation refuses.
const imports = { wasi_snapshot_preview1: wasi };
const ready = WebAssembly.instantiateStreaming(fetch("yan.wasm"), imports)
  .catch(() => fetch("yan.wasm").then((r) => r.arrayBuffer()).then((b) => WebAssembly.instantiate(b, imports)))
  .then(({ instance }) => {
    memory = instance.exports.memory;
    return instance.exports;
  });

function call(x, op, head, src) {
  const req = enc.encode(`${head}\n${src}`);
  const ptr = x.alloc(req.length);
  bytes(ptr, req.length).set(req);
  x[op](ptr, req.length);
  return JSON.parse(dec.decode(bytes(x.reply_ptr(), x.reply_len())));
}

onmessage = async ({ data: { id, op, head, src } }) => {
  try {
    postMessage({ id, ok: call(await ready, op, head, src) });
  } catch (e) {
    postMessage({ id, trap: String(e) });
  }
};
