import { h as head, c as store_get, e as escape_html, d as unsubscribe_stores } from "../../../chunks/index.js";
import { a as api } from "../../../chunks/api.js";
import "@sveltejs/kit/internal";
import "../../../chunks/exports.js";
import "../../../chunks/utils2.js";
import "@sveltejs/kit/internal/server";
import "../../../chunks/root.js";
import "../../../chunks/state.svelte.js";
import { c as createQuery } from "../../../chunks/createQuery.js";
function _page($$renderer, $$props) {
  $$renderer.component(($$renderer2) => {
    var $$store_subs;
    const user = createQuery({ queryKey: ["me"], queryFn: () => api.me(), retry: false });
    head("x1i5gj", $$renderer2, ($$renderer3) => {
      $$renderer3.title(($$renderer4) => {
        $$renderer4.push(`<title>Dashboard</title>`);
      });
    });
    $$renderer2.push(`<section>`);
    if (store_get($$store_subs ??= {}, "$user", user).isLoading) {
      $$renderer2.push("<!--[0-->");
      $$renderer2.push(`<p>Loading...</p>`);
    } else if (store_get($$store_subs ??= {}, "$user", user).isError) {
      $$renderer2.push("<!--[1-->");
      $$renderer2.push(`<p>Not authenticated. <a href="/login">Login</a></p>`);
    } else if (store_get($$store_subs ??= {}, "$user", user).data) {
      $$renderer2.push("<!--[2-->");
      $$renderer2.push(`<h1>Welcome, ${escape_html(store_get($$store_subs ??= {}, "$user", user).data.name)}</h1> <p>Email: ${escape_html(store_get($$store_subs ??= {}, "$user", user).data.email)}</p> <button>Logout</button>`);
    } else {
      $$renderer2.push("<!--[-1-->");
    }
    $$renderer2.push(`<!--]--></section>`);
    if ($$store_subs) unsubscribe_stores($$store_subs);
  });
}
export {
  _page as default
};
