import { eslintCompatPlugin } from "@oxlint/plugins";

import { noObjectParametersRule } from "./rules/no-object-parameters.ts";
import { noWidenThenAssertRule } from "./rules/no-widen-then-assert.ts";

/**
 * The project-owned subset of the generic anti-slop rules.
 *
 * Keep this entry point intentionally explicit: adding an upstream rule is a
 * policy decision that should be visible in the Oxlint configuration diff.
 */
const antiSlopPlugin = eslintCompatPlugin({
	meta: { name: "anti-slop" },
	rules: {
		"no-object-parameters": noObjectParametersRule,
		"no-widen-then-assert": noWidenThenAssertRule,
	},
});

export default antiSlopPlugin;
