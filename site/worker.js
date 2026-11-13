// Terminals get index.txt at /, over plain http too; browsers on http are sent to https.
// Accept decides first; the user agent covers clients that send only */*.
const TERMINAL = /^(curl|wget|httpie|xh)\b/i;

function terminal(request) {
  const accept = request.headers.get("accept") ?? "";
  if (/\btext\/html\b/.test(accept)) return false;
  return /\btext\/plain\b/.test(accept) || TERMINAL.test(request.headers.get("user-agent") ?? "");
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const text = terminal(request);
    if (url.protocol === "http:" && !text) {
      url.protocol = "https:";
      return new Response(null, { status: 301, headers: { location: url.href, vary: "accept, user-agent" } });
    }
    if (url.pathname !== "/") return env.ASSETS.fetch(request);
    const res = await env.ASSETS.fetch(text ? new URL("/index.txt", url) : request);
    const out = new Response(res.body, res);
    if (text) out.headers.set("content-type", "text/plain; charset=utf-8");
    out.headers.set("vary", "accept, user-agent");
    return out;
  },
};
