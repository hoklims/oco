export const manifest = (() => {
function __memo(fn) {
	let value;
	return () => value ??= (value = fn());
}

return {
	appDir: "_app",
	appPath: "_app",
	assets: new Set([]),
	mimeTypes: {},
	_: {
		client: {start:"_app/immutable/entry/start.D1fpIqw-.js",app:"_app/immutable/entry/app.7j88l_63.js",imports:["_app/immutable/entry/start.D1fpIqw-.js","_app/immutable/chunks/75GByQeU.js","_app/immutable/chunks/BLNif519.js","_app/immutable/chunks/DWcXEKEB.js","_app/immutable/entry/app.7j88l_63.js","_app/immutable/chunks/BLNif519.js","_app/immutable/chunks/BGPIp8_s.js","_app/immutable/chunks/g30nbZS4.js","_app/immutable/chunks/DWcXEKEB.js","_app/immutable/chunks/CsSOJsoF.js","_app/immutable/chunks/DtfPo9dt.js","_app/immutable/chunks/DG0uQ0ct.js"],stylesheets:[],fonts:[],uses_env_dynamic_public:false},
		nodes: [
			__memo(() => import('./nodes/0.js')),
			__memo(() => import('./nodes/1.js')),
			__memo(() => import('./nodes/2.js')),
			__memo(() => import('./nodes/3.js')),
			__memo(() => import('./nodes/4.js')),
			__memo(() => import('./nodes/5.js'))
		],
		remotes: {
			
		},
		routes: [
			{
				id: "/",
				pattern: /^\/$/,
				params: [],
				page: { layouts: [0,], errors: [1,], leaf: 2 },
				endpoint: null
			},
			{
				id: "/dashboard",
				pattern: /^\/dashboard\/?$/,
				params: [],
				page: { layouts: [0,], errors: [1,], leaf: 3 },
				endpoint: null
			},
			{
				id: "/login",
				pattern: /^\/login\/?$/,
				params: [],
				page: { layouts: [0,], errors: [1,], leaf: 4 },
				endpoint: null
			},
			{
				id: "/register",
				pattern: /^\/register\/?$/,
				params: [],
				page: { layouts: [0,], errors: [1,], leaf: 5 },
				endpoint: null
			}
		],
		prerendered_routes: new Set([]),
		matchers: async () => {
			
			return {  };
		},
		server_assets: {}
	}
}
})();
