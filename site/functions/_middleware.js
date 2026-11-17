// Terminal clients get the text page; browsers get HTML.
const TERMINAL = /^(curl|wget|httpie|xh|fetch)\b/i;

export async function onRequest({ request, next, env }) {
  const url = new URL(request.url);
  if (url.pathname === "/" && TERMINAL.test(request.headers.get("user-agent") ?? "")) {
    const res = await env.ASSETS.fetch(new URL("/index.txt", url));
    return new Response(res.body, { headers: { "content-type": "text/plain; charset=utf-8", vary: "user-agent" } });
  }
  const res = await next();
  if (url.pathname === "/") {
    const out = new Response(res.body, res);
    out.headers.set("vary", "user-agent");
    return out;
  }
  return res;
}
