import { h as head, c as store_get, e as escape_html, d as unsubscribe_stores } from "../../chunks/index.js";
import { a as api } from "../../chunks/api.js";
import { c as createQuery } from "../../chunks/createQuery.js";
function _page($$renderer, $$props) {
  $$renderer.component(($$renderer2) => {
    var $$store_subs;
    const health = createQuery({ queryKey: ["health"], queryFn: () => api.health() });
    head("1uha8ag", $$renderer2, ($$renderer3) => {
      $$renderer3.title(($$renderer4) => {
        $$renderer4.push(`<title>Fullstack Monorepo</title>`);
      });
    });
    $$renderer2.push(`<section><h1>Fullstack Monorepo</h1> <p>API Status: `);
    if (store_get($$store_subs ??= {}, "$health", health).isLoading) {
      $$renderer2.push("<!--[0-->");
      $$renderer2.push(`checking...`);
    } else if (store_get($$store_subs ??= {}, "$health", health).isError) {
      $$renderer2.push("<!--[1-->");
      $$renderer2.push(`<span style="color: red;">offline</span>`);
    } else {
      $$renderer2.push("<!--[-1-->");
      $$renderer2.push(`<span style="color: green;">${escape_html(store_get($$store_subs ??= {}, "$health", health).data?.status)}</span>`);
    }
    $$renderer2.push(`<!--]--></p> <nav><a href="/login">Login</a> | <a href="/register">Register</a> | <a href="/dashboard">Dashboard</a></nav></section>`);
    if ($$store_subs) unsubscribe_stores($$store_subs);
  });
}
export {
  _page as default
};
