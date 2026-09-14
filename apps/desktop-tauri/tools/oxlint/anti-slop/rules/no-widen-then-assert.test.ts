import { RuleTester } from "oxlint/plugins-dev";

import { noWidenThenAssertRule } from "./no-widen-then-assert.ts";

const tester = new RuleTester({ languageOptions: { parserOptions: { lang: "ts" } } });
const error = { messageId: "widenThenAssert" };

tester.run("anti-slop/no-widen-then-assert", noWidenThenAssertRule, {
	valid: [
		"const source = { id: 'first' }; const widened: unknown = source;",
		"declare const input: unknown; const parsed = input as { readonly id: string };",
		"type Record = { readonly id: string }; const source = { id: 'third' }; const widened: Record = source; const parsed = widened as { readonly id: string };",
		"type A = B; type B = A; const source = { id: 'cycle' }; const widened: A = source; const parsed = widened as { readonly id: string };",
		"type Record = string; function parse() { type Record = { readonly id: string }; const source = { id: 'nested' }; const widened: Record = source; const parsed = widened as { readonly id: string }; }",
	],
	invalid: [
		{
			code: "const source = { id: 'second' }; const widened: unknown = source; const parsed = widened as { readonly id: string };",
			errors: [error],
		},
		{
			code: "const source = { id: 'second' }; const widened: object = source; const parsed = widened as { readonly id: string };",
			errors: [error],
		},
		{
			code: "type Unknown = unknown; const source = { id: 'third' }; const widened: Unknown = source; const parsed = widened as { readonly id: string };",
			errors: [error],
		},
		{
			code: "interface Payload { readonly id: string } const source = { id: 'fourth' }; const widened: object = source; const parsed = widened as Payload;",
			errors: [error],
		},
		{
			code: "type Payload = { readonly id: string }; const source = { id: 'fifth' }; const widened: object = source; const parsed = widened as Payload;",
			errors: [error],
		},
		{
			code: "type Unknown = unknown; type MaybeUnknown = Unknown; const source = { id: 'sixth' }; const widened: MaybeUnknown = source; const parsed = widened as { readonly id: string };",
			errors: [error],
		},
		{
			code: "const source = { id: 'seventh' }; const widened: Record<string, unknown> = source; const parsed = widened as { readonly id: string };",
			errors: [error],
		},
	],
});
