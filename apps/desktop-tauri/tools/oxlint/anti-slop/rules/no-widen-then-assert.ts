import { defineRule } from "@oxlint/plugins";
import type { ESTree, Variable } from "@oxlint/plugins";

import {
  createTypeAliasEnvironment,
  hasVisibleTypeBinding,
  resolvedTypeMatches,
  visibleTypeBinding,
  type TypeAliasEnvironment,
} from "../shared/type-alias-resolution.ts";

type BroadTypeKind = "top" | "object" | "record";

type KnownValueEvidence = {
  readonly type: ESTree.TSType | null;
};

const functionBoundaryTypes = new Set([
  "ArrowFunctionExpression",
  "FunctionDeclaration",
  "FunctionExpression",
  "TSDeclareFunction",
  "TSEmptyBodyFunctionExpression",
]);

function unwrapExpressionParentheses(expression: ESTree.Expression): ESTree.Expression {
  let current = expression;
  while (current.type === "ParenthesizedExpression") current = current.expression;
  return current;
}

function unwrapTypeParentheses(type: ESTree.TSType): ESTree.TSType {
  let current = type;
  while (current.type === "TSParenthesizedType") current = current.typeAnnotation;
  return current;
}

function typeReferenceName(type: ESTree.TSTypeReference): string | null {
  return type.typeName.type === "Identifier" ? type.typeName.name : null;
}

function isUnknownOrAnyType(type: ESTree.TSType): boolean {
  const unwrapped = unwrapTypeParentheses(type);
  return unwrapped.type === "TSUnknownKeyword" || unwrapped.type === "TSAnyKeyword";
}

function isBroadRecordKeyType(
  type: ESTree.TSType,
  environment: TypeAliasEnvironment,
): boolean {
  return resolvedTypeMatches(type, environment, (current) => {
    const unwrapped = unwrapTypeParentheses(current);
    if (
      unwrapped.type === "TSStringKeyword" ||
      unwrapped.type === "TSNumberKeyword" ||
      unwrapped.type === "TSSymbolKeyword"
    ) {
      return true;
    }
    if (unwrapped.type === "TSUnionType") {
      return unwrapped.types.every((member) => isBroadRecordKeyType(member, environment));
    }
    return (
      unwrapped.type === "TSTypeReference" &&
      typeReferenceName(unwrapped) === "PropertyKey" &&
      !hasVisibleTypeBinding("PropertyKey", unwrapped, environment)
    );
  });
}

function isBroadRecordType(
  type: ESTree.TSType,
  environment: TypeAliasEnvironment,
  matches: (child: ESTree.TSType) => boolean,
): boolean {
  const unwrapped = unwrapTypeParentheses(type);

  if (unwrapped.type === "TSTypeReference") {
    const name = typeReferenceName(unwrapped);
    if (name === "Readonly" && !hasVisibleTypeBinding(name, unwrapped, environment)) {
      const [inner] = unwrapped.typeArguments?.params ?? [];
      return inner !== undefined && matches(inner);
    }

    if (
      name !== "Record" ||
      hasVisibleTypeBinding("Record", unwrapped, environment)
    ) {
      return false;
    }
    const parameters = unwrapped.typeArguments?.params ?? [];
    return (
      parameters.length === 2 &&
      parameters[0] !== undefined &&
      parameters[1] !== undefined &&
      isBroadRecordKeyType(parameters[0], environment) &&
      resolvedTypeMatches(parameters[1], environment, (current) =>
        isUnknownOrAnyType(current),
      )
    );
  }

  if (unwrapped.type !== "TSTypeLiteral" || unwrapped.members.length !== 1) return false;
  const [member] = unwrapped.members;
  const [parameter] = member?.type === "TSIndexSignature" ? member.parameters : [];
  return (
    member?.type === "TSIndexSignature" &&
    member.parameters.length === 1 &&
    parameter !== undefined &&
    isBroadRecordKeyType(parameter.typeAnnotation.typeAnnotation, environment) &&
    resolvedTypeMatches(member.typeAnnotation.typeAnnotation, environment, (current) =>
      isUnknownOrAnyType(current),
    )
  );
}

function broadTypeKind(
  type: ESTree.TSType,
  environment: TypeAliasEnvironment,
): BroadTypeKind | null {
  if (
    resolvedTypeMatches(type, environment, (current) => {
      const unwrapped = unwrapTypeParentheses(current);
      return unwrapped.type === "TSUnknownKeyword" || unwrapped.type === "TSAnyKeyword";
    })
  ) {
    return "top";
  }
  if (
    resolvedTypeMatches(
      type,
      environment,
      (current) => unwrapTypeParentheses(current).type === "TSObjectKeyword",
    )
  ) {
    return "object";
  }
  if (
    resolvedTypeMatches(type, environment, (current, matches) =>
      isBroadRecordType(current, environment, matches),
    )
  ) {
    return "record";
  }
  return null;
}

function assertedExpression(
  node: ESTree.TSAsExpression | ESTree.TSTypeAssertion,
): ESTree.Expression {
  return unwrapExpressionParentheses(node.expression);
}

function assertionFromExpression(
  expression: ESTree.Expression,
): ESTree.TSAsExpression | ESTree.TSTypeAssertion | null {
  const unwrapped = unwrapExpressionParentheses(expression);
  return unwrapped.type === "TSAsExpression" || unwrapped.type === "TSTypeAssertion"
    ? unwrapped
    : null;
}

function normalizedTypeText(sourceText: string, type: ESTree.TSType): string {
  return sourceText.slice(type.start, type.end).replace(/\s+/gu, "");
}

function typesHaveSameSyntax(
  sourceText: string,
  left: ESTree.TSType | null,
  right: ESTree.TSType,
): boolean {
  return (
    left !== null &&
    normalizedTypeText(sourceText, unwrapTypeParentheses(left)) ===
      normalizedTypeText(sourceText, unwrapTypeParentheses(right))
  );
}

function visibleNamedObjectType(
  type: ESTree.TSType,
  environment: TypeAliasEnvironment,
): boolean {
  const unwrapped = unwrapTypeParentheses(type);
  if (unwrapped.type !== "TSTypeReference") return false;
  const name = typeReferenceName(unwrapped);
  if (name === null) return false;
  const binding = visibleTypeBinding(name, unwrapped, environment);
  return (
    binding?.declaration.type === "TSInterfaceDeclaration" ||
    binding?.declaration.type === "ClassDeclaration" ||
    binding?.declaration.type === "ClassExpression"
  );
}

function isDefinitelyObjectType(
  type: ESTree.TSType,
  environment: TypeAliasEnvironment,
): boolean {
  return resolvedTypeMatches(type, environment, (current, matches) => {
    const unwrapped = unwrapTypeParentheses(current);
    switch (unwrapped.type) {
      case "TSArrayType":
      case "TSConstructorType":
      case "TSFunctionType":
      case "TSMappedType":
      case "TSObjectKeyword":
      case "TSTupleType":
        return true;
      case "TSTypeLiteral":
        return unwrapped.members.length > 0;
      case "TSIntersectionType":
        return unwrapped.types.every((member) => matches(member));
      case "TSTypeOperator":
        return unwrapped.operator === "readonly" && matches(unwrapped.typeAnnotation);
      case "TSTypeReference":
        return (
          typeReferenceName(unwrapped) === "Readonly" &&
          !hasVisibleTypeBinding("Readonly", unwrapped, environment) &&
          unwrapped.typeArguments?.params[0] !== undefined &&
          matches(unwrapped.typeArguments.params[0])
        ) || visibleNamedObjectType(unwrapped, environment);
      default:
        return false;
    }
  });
}

function isDefinitelyNarrowerRecordType(
  type: ESTree.TSType,
  environment: TypeAliasEnvironment,
): boolean {
  return resolvedTypeMatches(type, environment, (current, matches) => {
    const unwrapped = unwrapTypeParentheses(current);
    if (unwrapped.type === "TSTypeLiteral") {
      return unwrapped.members.some((member) => member.type !== "TSIndexSignature");
    }
    if (unwrapped.type !== "TSTypeReference") return false;

    const name = typeReferenceName(unwrapped);
    if (visibleNamedObjectType(unwrapped, environment)) return true;
    if (
      name === "Readonly" &&
      !hasVisibleTypeBinding(name, unwrapped, environment)
    ) {
      const [inner] = unwrapped.typeArguments?.params ?? [];
      return inner !== undefined && matches(inner);
    }
    if (
      name !== "Record" ||
      hasVisibleTypeBinding("Record", unwrapped, environment)
    ) {
      return false;
    }

    const parameters = unwrapped.typeArguments?.params ?? [];
    return (
      parameters.length === 2 &&
      parameters[1] !== undefined &&
      !resolvedTypeMatches(parameters[1], environment, (member) =>
        isUnknownOrAnyType(member),
      )
    );
  });
}

function functionBoundary(node: ESTree.Node): ESTree.Node | null {
  let current = node.parent;
  while (current !== null && current.type !== "Program") {
    if (functionBoundaryTypes.has(current.type)) return current;
    current = current.parent;
  }
  return null;
}

function resolvedVariableForIdentifier(
  scopes: readonly {
    readonly references: readonly {
      readonly identifier: ESTree.Node;
      readonly resolved: Variable | null;
    }[];
  }[],
  identifier: ESTree.IdentifierReference,
): Variable | null {
  for (const scope of scopes) {
    const reference = scope.references.find(
      (candidate) =>
        candidate.identifier.start === identifier.start &&
        candidate.identifier.end === identifier.end,
    );
    if (reference !== undefined) return reference.resolved;
  }
  return null;
}

function variableDeclarator(variable: Variable): ESTree.VariableDeclarator | null {
  for (const definition of variable.defs) {
    if (definition.type === "Variable" && definition.node.type === "VariableDeclarator") {
      return definition.node;
    }
  }
  return null;
}

function knownValueEvidence(
  expression: ESTree.Expression,
  scopes: Parameters<typeof resolvedVariableForIdentifier>[0],
  boundary: ESTree.Node | null,
  visitedVariables: ReadonlySet<Variable>,
  environment: TypeAliasEnvironment,
): KnownValueEvidence | null {
  const unwrapped = unwrapExpressionParentheses(expression);

  if (unwrapped.type === "TSAsExpression" || unwrapped.type === "TSTypeAssertion") {
    if (broadTypeKind(unwrapped.typeAnnotation, environment) !== null) return null;
    return { type: unwrapped.typeAnnotation };
  }

  if (unwrapped.type === "Literal" || unwrapped.type === "TemplateLiteral") {
    return { type: null };
  }

  if (
    unwrapped.type === "ArrayExpression" ||
    unwrapped.type === "ArrowFunctionExpression" ||
    unwrapped.type === "ClassExpression" ||
    unwrapped.type === "FunctionExpression" ||
    unwrapped.type === "NewExpression" ||
    unwrapped.type === "ObjectExpression"
  ) {
    return { type: null };
  }

  if (unwrapped.type !== "Identifier") return null;
  const variable = resolvedVariableForIdentifier(scopes, unwrapped);
  if (variable === null || visitedVariables.has(variable)) return null;

  const annotatedIdentifier = variable.identifiers.find(
    (identifier) => identifier.typeAnnotation !== null && identifier.typeAnnotation !== undefined,
  );
  const annotation = annotatedIdentifier?.typeAnnotation?.typeAnnotation;
  if (annotation !== undefined && annotatedIdentifier !== undefined) {
    if (
      functionBoundary(annotatedIdentifier) !== boundary ||
      broadTypeKind(annotation, environment) !== null
    ) {
      return null;
    }
    return { type: annotation };
  }

  const declarator = variableDeclarator(variable);
  if (
    declarator === null ||
    declarator.parent.type !== "VariableDeclaration" ||
    declarator.parent.kind !== "const" ||
    declarator.init === null ||
    variable.references.some((reference) => reference.isWrite() && !reference.init) ||
    functionBoundary(declarator) !== boundary
  ) {
    return null;
  }

  return knownValueEvidence(
    declarator.init,
    scopes,
    boundary,
    new Set([...visitedVariables, variable]),
    environment,
  );
}

function widenedBinding(
  variable: Variable,
  scopes: Parameters<typeof resolvedVariableForIdentifier>[0],
  environment: TypeAliasEnvironment,
): {
  readonly broadKind: BroadTypeKind;
  readonly evidence: KnownValueEvidence;
  readonly declaredAt: number;
  readonly boundary: ESTree.Node | null;
} | null {
  const declarator = variableDeclarator(variable);
  if (
    declarator === null ||
    declarator.parent.type !== "VariableDeclaration" ||
    declarator.parent.kind !== "const" ||
    declarator.id.type !== "Identifier" ||
    declarator.init === null ||
    variable.references.some((reference) => reference.isWrite() && !reference.init)
  ) {
    return null;
  }

  const boundary = functionBoundary(declarator);
  const declaredType = declarator.id.typeAnnotation?.typeAnnotation;
  const initializerAssertion = assertionFromExpression(declarator.init);
  const initializerBroadKind =
    initializerAssertion === null
      ? null
      : broadTypeKind(initializerAssertion.typeAnnotation, environment);
  const declaredBroadKind =
    declaredType === undefined ? null : broadTypeKind(declaredType, environment);
  const broadKind = declaredBroadKind ?? initializerBroadKind;
  if (broadKind === null) return null;

  const originalExpression =
    initializerAssertion !== null && initializerBroadKind !== null
      ? assertedExpression(initializerAssertion)
      : declarator.init;
  const evidence = knownValueEvidence(
    originalExpression,
    scopes,
    boundary,
    new Set([variable]),
    environment,
  );
  return evidence === null ? null : { broadKind, evidence, declaredAt: declarator.end, boundary };
}

function assertionIsNarrower(
  sourceText: string,
  broadKind: BroadTypeKind,
  evidence: KnownValueEvidence,
  assertedType: ESTree.TSType,
  environment: TypeAliasEnvironment,
): boolean {
  if (broadTypeKind(assertedType, environment) !== null) return false;
  if (broadKind === "top") return true;
  if (typesHaveSameSyntax(sourceText, evidence.type, assertedType)) return true;
  if (broadKind === "object") return isDefinitelyObjectType(assertedType, environment);
  return isDefinitelyNarrowerRecordType(assertedType, environment);
}

/** Detect immutable local bindings that erase a known type and are later asserted back to a narrower type. */
export const noWidenThenAssertRule = defineRule({
  meta: {
    type: "problem",
    docs: {
      description:
        "Disallow local const flows that explicitly widen a known value before asserting the widened binding to a narrower type.",
    },
    messages: {
      widenThenAssert:
        'Binding "{{name}}" discards type evidence and later recreates it with an assertion. Keep the precise type from initialization through use; parse boundary input once.',
    },
  },
  createOnce(context) {
    let scopes: Parameters<typeof resolvedVariableForIdentifier>[0] = [];
    let environment: TypeAliasEnvironment | null = null;

    const checkAssertion = (node: ESTree.TSAsExpression | ESTree.TSTypeAssertion) => {
      const expression = assertedExpression(node);
      if (expression.type !== "Identifier") return;

      const variable = resolvedVariableForIdentifier(scopes, expression);
      if (variable === null) return;
      const activeEnvironment = environment;
      if (activeEnvironment === null) return;
      const widened = widenedBinding(variable, scopes, activeEnvironment);
      if (
        widened === null ||
        node.start <= widened.declaredAt ||
        functionBoundary(node) !== widened.boundary ||
        !assertionIsNarrower(
          context.sourceCode.text,
          widened.broadKind,
          widened.evidence,
          node.typeAnnotation,
          activeEnvironment,
        )
      ) {
        return;
      }

      context.report({
        node,
        messageId: "widenThenAssert",
        data: { name: expression.name },
      });
    };

    return {
      Program(node) {
        scopes = context.sourceCode.scopeManager.scopes;
        environment = createTypeAliasEnvironment(
          node,
          context.sourceCode.visitorKeys,
        );
      },
      TSAsExpression: checkAssertion,
      TSTypeAssertion: checkAssertion,
    };
  },
});
