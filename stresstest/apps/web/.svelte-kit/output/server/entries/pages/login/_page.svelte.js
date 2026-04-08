import { h as head, i as attr, c as store_get, e as escape_html, d as unsubscribe_stores } from "../../../chunks/index.js";
import { c as createMutation } from "../../../chunks/user.js";
import { a as api } from "../../../chunks/api.js";
import { g as goto } from "../../../chunks/client.js";
function _page($$renderer, $$props) {
  $$renderer.component(($$renderer2) => {
    var $$store_subs;
    let email = "";
    let password = "";
    let error = "";
    const login = createMutation({
      mutationFn: api.login,
      onSuccess: (data) => {
        localStorage.setItem("token", data.token);
        goto();
      },
      onError: (err) => {
        error = err.message;
      }
    });
    head("1x05zx6", $$renderer2, ($$renderer3) => {
      $$renderer3.title(($$renderer4) => {
        $$renderer4.push(`<title>Login</title>`);
      });
    });
    $$renderer2.push(`<section><h1>Login</h1> <form><label>Email <input type="email"${attr("value", email)} required=""/></label> <label>Password <input type="password"${attr("value", password)} required=""/></label> `);
    if (error) {
      $$renderer2.push("<!--[0-->");
      $$renderer2.push(`<p style="color: red;">${escape_html(error)}</p>`);
    } else {
      $$renderer2.push("<!--[-1-->");
    }
    $$renderer2.push(`<!--]--> <button type="submit"${attr("disabled", store_get($$store_subs ??= {}, "$login", login).isPending, true)}>${escape_html(store_get($$store_subs ??= {}, "$login", login).isPending ? "Logging in..." : "Login")}</button></form> <p><a href="/register">Don't have an account? Register</a></p></section>`);
    if ($$store_subs) unsubscribe_stores($$store_subs);
  });
}
export {
  _page as default
};
