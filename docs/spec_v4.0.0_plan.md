# Spec v4.0.0 Implementation Plan

## Goals
Spec v4.0.0 extends the mini-program format to improve expressiveness, fault tolerance, and metadata for LLM-driven tooling. The release must remain backward compatible with v3.0.0 documents while introducing new optional sections that tooling can gradually adopt.

## Guiding Principles
- **Declarative-first:** New features should be described declaratively in the spec, minimizing imperative glue code.
- **Backward compatibility:** Existing v3.0.0 documents must stay valid. All additions are optional or guarded by a version bump.
- **Schema clarity:** Every new structure should include JSON Schema definitions, examples, and validation rules.
- **Tooling impact:** For each feature, document the minimal changes required in `program-verify` and downstream compilers/interpreters.

## Feature Breakdown

### 1. Declarative Control Flow Graph
- **Schema additions:**
  - Replace `algorithm.phases` array with an object that contains both `nodes` (phase definitions) and `edges` (control-flow transitions).
  - Introduce node types: `phase`, `if`, `loop`, `parallel`, each with specific constraints.
  - Allow specifying entry node and optional termination conditions.
- **Validation rules:**
  - Ensure graph connectivity and absence of orphan nodes.
  - Validate type-specific fields (e.g., `if` nodes require `condition` and branch targets; `loop` nodes require `body` and `until`).
- **Migration considerations:**
  - Provide automatic conversion path for simple linear sequences.
  - Maintain support for a legacy list via compatibility shim when `nodes` is absent.
- **Tooling updates:**
  - Update execution planner to traverse graph, support branching, and parallel execution hints.
  - Extend docs with visual examples of control-flow graphs.

### 2. Error Model and Retry Policies
- **Schema additions:**
  - Extend phase contracts with optional `errors` block listing known error codes, descriptions, and severity.
  - Add `retry_policy` object per phase (e.g., max attempts, backoff strategy, retryable errors).
  - Support `fallback` referencing alternate phases or global handlers.
- **Validation rules:**
  - Enforce unique error codes within a phase.
  - Require referenced fallback phases to exist in the control-flow graph.
- **Tooling updates:**
  - Update runtime guidance to honor retry policies and route to fallbacks.
  - Provide diagnostics when an execution result does not match declared errors.

### 3. Output Composition Mechanisms
- **Schema additions:**
  - Introduce `algorithm.outputs` section that defines named aggregates built from multiple phase outputs.
  - Allow declarative transformations (e.g., concatenation, struct assembly) using a small expression language or template references.
  - Support typing information for composed outputs.
- **Validation rules:**
  - Verify that referenced phase outputs exist and types are compatible.
  - Detect cycles in composition definitions.
- **Tooling updates:**
  - Extend code generation to materialize composed outputs automatically.
  - Provide examples demonstrating usage for report generation and dataset creation.

### 4. Semantic Phase Contracts
- **Schema additions:**
  - Add `semantics` metadata to phases including `category` (classification, extraction, transformation, etc.), `capabilities`, and `quality_metrics` expectations.
  - Allow linking to external ontologies or taxonomy URIs.
- **Validation rules:**
  - Maintain controlled vocabulary for `category`; allow extensions via `x-` fields.
  - Validate metric definitions (name, target range, units).
- **Tooling updates:**
  - Map semantics to recommended prompt templates or model configurations.
  - Surface semantic metadata in documentation generators.

### 5. Versioned Sources and Artifacts
- **Schema additions:**
  - Extend `implementation.source` entries to support structured objects with `uri`, `type` (code, prompt, model), `version`, `checksum`, and optional `integrity` metadata.
  - Allow referencing external artifact registries.
- **Validation rules:**
  - Require at least `uri` and either `version` or `checksum` for reproducibility.
  - Enforce consistent versioning scheme (semver or git SHA) based on `type`.
- **Tooling updates:**
  - Update fetch/build pipeline to resolve versioned artifacts.
  - Provide warnings when integrity metadata is missing.

## Cross-Cutting Tasks
- Update `specification.json` with draft v4.0.0 schema definitions.
- Add new examples under `examples/v4.0.0/` covering control flow, retries, compositions, and semantic metadata.
- Update `version_map.yaml` to register the new schema.
- Document migration steps from v3.0.0 in README and CHANGELOG.
- Expand automated tests to validate new schema features and backward compatibility.

## Timeline (Rough)
1. **Week 1:** Draft schema changes for control flow and error model; build validation tests.
2. **Week 2:** Implement output composition and semantic metadata sections; update tooling prototypes.
3. **Week 3:** Finalize versioned artifact support, write migration docs, and polish examples.
4. **Week 4:** Freeze schema, update version map, complete QA, and publish v4.0.0 release notes.

## Open Questions
- What expression language should power output compositions? Investigate using JSONPath or a restricted JMESPath subset.
- Should retry policies be standardized across tooling, or allow custom plugins per phase?
- Determine minimal semantics taxonomy to seed (`classification`, `generation`, `extraction`, `routing`, etc.).

## Acceptance Criteria
- Schema passes JSON Schema validation tooling with no errors.
- Sample specs using new features validate and execute in reference tooling.
- Documentation clearly outlines new structures with diagrams and examples.
- Versioned artifacts can be resolved deterministically in CI pipelines.
