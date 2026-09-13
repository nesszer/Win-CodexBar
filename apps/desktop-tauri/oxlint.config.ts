import { defineConfig } from "oxlint";

export default defineConfig({
	ignorePatterns: ["tools/oxlint/anti-slop/**"],
	jsPlugins: [
		{ name: "anti-slop", specifier: "./tools/oxlint/anti-slop/index.ts" },
	],
	rules: {
		"anti-slop/no-object-parameters": "error",
		"anti-slop/no-widen-then-assert": "error",
	},
});
