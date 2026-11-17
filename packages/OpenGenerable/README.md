# OpenGenerable

Guided, structured generation for Swift on Linux: define a schema, give it to a local model, get back a typed, validated result. It recreates the developer experience of Apple's `DynamicGenerationSchema` and (later) `@Generable` for local models and inference servers (llama.cpp, Ollama, vLLM). It does not port or link Apple's implementation, and nothing in it needs an Apple framework.

The package has no dependencies and nothing from ixOS, so it can move to its own repository once the boundary has proved useful. Until then it lives here, in the framework area ([areas.md](../../docs/areas.md)). "OpenGenerable" is a working name.

## What M1 gives you

The dynamic schema core, fully testable without a model:

- **Schemas built at run time**: objects with ordered properties, strings, integers and numbers (with bounds), booleans, null, lists (with lengths), choices (`anyOf` strings), alternatives (`anyOf` schemas), and references to named schemas, so schemas can be recursive.
- **Checked when created**: `GenerationSchema(root:dependencies:)` refuses unresolved references, two different schemas with one name, unnamed dependencies, duplicate properties, empty choices, impossible bounds, and recursion with no way to end (a model would write forever).
- **Validation** of a model's answer: every violation, each with a JSON Pointer path (`/inventory/0/weight`) and a reason. Invalid values cannot pass.
- **JSON Schema export** (draft 2020-12): properties in declared order, closed objects, `$defs` and `$ref` for references.
- **`GeneratedContent`**: the answer as JSON with keys in written order, a strict parser, accessors, and `decode(_:)` into any `Decodable` type.

```swift
import OpenGenerable

let item = DynamicGenerationSchema(
    name: "Item",
    properties: [
        .init(name: "name", schema: .string()),
        .init(name: "weight", schema: .number(range: 0...50)),
    ])

let character = DynamicGenerationSchema(
    name: "Character",
    description: "A fantasy RPG character",
    properties: [
        .init(name: "name", schema: .string(description: "Character name")),
        .init(name: "class", schema: .anyOf(["Warrior", "Mage", "Rogue"])),
        .init(name: "level", schema: .integer(range: 1...20)),
        .init(name: "inventory", schema: .array(of: item, count: 0...3)),
        .init(name: "motto", schema: .string(), isOptional: true),
    ])

let schema = try GenerationSchema(root: character)
print(schema.jsonSchemaString)            // for llama.cpp, Ollama, vLLM

let answer = try schema.content(fromJSON: modelOutput)   // throws ValidationError
let level = answer["level"]?.intValue
```

Apple's initialisers work too: `DynamicGenerationSchema(name:description:properties:)`, `(name:description:anyOf:)` with strings or schemas, `(arrayOf:minimumElements:maximumElements:)` and `(referenceTo:)`, and `Property(name:description:schema:isOptional:)`.

## Differences from Apple's API

This is a compatibility layer, not a promise of identical behaviour.

- **Shorthand.** `.string()`, `.integer(range:)`, `.number(range:)`, `.boolean()`, `.null`, `.array(of:count:)`, `.anyOf(_:)`, `.object(...)` and `.reference(to:)` are additions. Apple builds primitives with `DynamicGenerationSchema(type:guides:)`, which needs `Generable` types and `GenerationGuide`; that arrives with M4.
- **Not yet:** string patterns (regex guides), constant strings, and any guide beyond bounds and lengths. They are M4 work, since each needs a grammar in M2 too.
- **Numbers** are `Double`, as in Apple's `GeneratedContent`; an integer schema accepts any whole number.
- **Optional means the key may be left out**, and nothing more. A present value must fit the property's schema, so `null` passes only where the schema allows it: write `.anyOf([schema, .null])` for a nullable value. The validator and the exported JSON Schema (where an optional property is simply not `required`) describe the same contract, which M2 relies on, since it constrains the model from the export.
- **Inference stays out of this package.** Backends (llama.cpp, Ollama, vLLM) go in separate adapter targets or packages that depend on this one, so the schema core keeps no dependencies.

## Plan

One pull request per milestone, in this order.

| Milestone | What | State |
|---|---|---|
| M1 | Dynamic schema core: schemas, validation, nested objects, lists, choices, references, JSON Schema export. No model needed | This package |
| M2 | Guided generation with llama.cpp: grammar-constrained decoding from the schema, and a test that the output really obeys it. A prompt-and-validate fallback for backends without grammars, marked as weaker | Next |
| M3 | Streaming: partially generated structured content as it arrives, cancellation, incomplete results | |
| M4 | `@Generable` and `@Guide` macros (SwiftSyntax, builds on Linux), compile-time schemas, more guides | |
| M5 | ixOS: use it in Device Lab's AI Lab next to Apple's Foundation Models and compare. The AI Lab's `StructuredShape` is replaced by this package here | |

The first end-to-end proof (after M3): on Ubuntu, define a dynamic schema, generate a nested object with a local model, stream the partial result, and show that invalid values cannot pass validation.

## Checking it

On Linux with Swift 6:

```bash
cd packages/OpenGenerable
swift test
```

CI runs the same on Ubuntu (`.github/workflows/opengenerable.yml`).

## Licence

MIT OR Apache-2.0, like the rest of ixOS.
