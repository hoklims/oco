import { B as getQueryClientContext } from "./context.js";
function useQueryClient(queryClient) {
  return getQueryClientContext();
}
function isSvelteStore(obj) {
  return "subscribe" in obj && typeof obj.subscribe === "function";
}
const BASE = "/api";
async function request(path, init) {
  const token = typeof localStorage !== "undefined" ? localStorage.getItem("token") : null;
  const headers = {
    "Content-Type": "application/json",
    ...token ? { Authorization: `Bearer ${token}` } : {}
  };
  const res = await fetch(`${BASE}${path}`, { ...init, headers });
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw new Error(body.error ?? `Request failed: ${res.status}`);
  }
  return res.json();
}
const api = {
  login: (data) => request("/auth/login", {
    method: "POST",
    body: JSON.stringify(data)
  }),
  register: (data) => request("/auth/register", {
    method: "POST",
    body: JSON.stringify(data)
  }),
  me: () => request("/auth/me"),
  health: () => request("/health")
};
export {
  api as a,
  isSvelteStore as i,
  useQueryClient as u
};
