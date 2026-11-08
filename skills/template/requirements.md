# Rust Component Requirements

- **Date:** YYYY-MM-DD
- **Author:** `{{GIT_USERNAME}}` (from `git config user.name`)
- **Revision:** 0.1
- **Short description:** A concise summary of the component and its purpose.

## Table of contents

- [Overview](#overview)
- [Goals and scope](#goals-and-scope)
- [Assumptions and dependencies](#assumptions-and-dependencies)
- [Requirements](#requirements)
- [Public API](#public-api)
- [Errors and failure behavior](#errors-and-failure-behavior)
- [Safety and security](#safety-and-security)
- [Compatibility](#compatibility)
- [Performance and resource use](#performance-and-resource-use)
- [Phases](#phases)
  - [Phase planning and reflection](#phase-planning-and-reflection)
  - [Current implementation checklist](#current-implementation-checklist)
- [Testing and acceptance criteria](#testing-and-acceptance-criteria)
- [Code coverage](#code-coverage)
- [Out of scope](#out-of-scope)

## Overview

Describe the component, the problem it addresses, and its intended users.

## Goals and scope

### Goals

- State the outcomes this component must provide.

### Scope

- Identify the modules, types, and behaviors covered by these requirements.
- Note existing functionality that must remain unchanged.

## Assumptions and dependencies

- Record assumptions about the runtime environment, inputs, users, and deployment.
- List required crates, external services, and relevant feature flags.
- Specify the minimum supported Rust version (MSRV), if applicable.

## Requirements

### Functional requirements

Use one row per requirement. Prefix each ID with a short, recognizable
identifier for the project, followed by its category and a zero-padded
sequence number (for example, `MYPROJ-FR-001`). Use the same project prefix
throughout this document so IDs are unique, stable, and easy to reference from
tests and acceptance criteria.

| ID | Requirement |
|---|---|
| `{{PROJECT_PREFIX}}-FR-001` | Describe an observable behavior the component must provide. |
| `{{PROJECT_PREFIX}}-FR-002` | Describe expected behavior for relevant inputs, boundary cases, and state changes. |

### Data and serialization

| ID | Requirement |
|---|---|
| `{{PROJECT_PREFIX}}-DS-001` | Specify data models, invariants, formats, and compatibility requirements, if applicable. |
| `{{PROJECT_PREFIX}}-DS-002` | Define how invalid, missing, or incomplete data is handled. |

### Rust-specific requirements

| ID | Requirement |
|---|---|
| `{{PROJECT_PREFIX}}-RS-001` | Specify required traits, ownership and borrowing behavior, lifetimes, and generic bounds. |
| `{{PROJECT_PREFIX}}-RS-002` | Define collection semantics and indexing behavior, if applicable. |
| `{{PROJECT_PREFIX}}-RS-003` | State the Rust edition, MSRV, and any relevant platform requirements. |

## Public API

- List required public modules, types, traits, functions, and methods.
- Specify inputs, outputs, visibility, and whether operations are fallible.
- Describe usage examples or API compatibility constraints, if applicable.

## Errors and failure behavior

- List failures callers must be able to observe and how they are reported.
- Define behavior for invalid arguments and unavailable resources.
- Specify whether cleanup or persistence can fail and how callers observe those failures.
- Do not allow failures to be silently converted into success-shaped results.

## Safety and security

- State whether `unsafe` code is allowed and what justification or review it requires.
- Specify input validation and handling of untrusted data.
- Identify sensitive data and requirements for its storage, exposure, and cleanup.
- Record any other security-sensitive behavior or threat assumptions relevant to the component.

## Compatibility

- Identify existing APIs and behavior that must be preserved.
- State any required compatibility with data formats, dependencies, or other components.
- Describe migration requirements if behavior or APIs intentionally change.

## Performance and resource use

- State measurable performance, memory, buffering, or resource-use constraints, if any.
- Describe concurrency assumptions and resource ownership.
- Mark unspecified performance characteristics as non-goals rather than implying guarantees.

## Phases

### Phase planning and reflection

Plan phases for this project rather than using a fixed, universal sequence.
Before implementation begins, describe how the work should be divided and
consider dependencies, sequencing, risks, and completion criteria. Add, remove,
split, merge, or reorder phases as the project requires. Revisit the plan as
implementation progresses and record meaningful changes or lessons learned.

| Phase ID | Phase and implementation steps | Dependencies | Completion criteria |
|---|---|---|---|
| P-001 | Describe the project-specific phase and the implementation steps it includes. | List prerequisite phases or `None`. | State measurable or verifiable conditions for completing this phase. |

### Current implementation checklist

Keep this checklist synchronized with the planned phases above. Add one item per
phase and update its state as work progresses. Check the box only when the phase
is complete; use the status label to distinguish phases that are not started,
in progress, or blocked. Record blockers in the item.

- [ ] P-001 — `{{PHASE_NAME}}` — Not started

**Planning reflection (complete before implementation):**

- Why is this phase breakdown appropriate for the project?
- Are the phase order and dependencies clear, and can phases be implemented and verified independently where practical?
- What risks, unknowns, or decisions could change the phase plan?

**Implementation reflection (update as work progresses):**

- Does the current plan still match the work and newly discovered constraints?
- Should any phase be added, removed, split, merged, or reordered? Record the reason.
- At phase completion, were its completion criteria met? Note deviations and lessons learned.

## Testing and acceptance criteria

### Tests

List every unit, integration, and other test that must be implemented and run to
verify these requirements. Include relevant edge cases such as serialization,
errors, boundaries, and compatibility. Add one row per test, link it to the
requirement IDs it verifies, and fill in the specific test file and test name.
Use **⏳ Not run** until the test has been executed, then set the status to
**✅ Success** or **❌ Failed**.

| Test ID | Requirement IDs | Description of the test | Success requirements | Test file | Test name | Status |
|---|---|---|---|---|---|---|
| T-001 | `{{PROJECT_PREFIX}}-FR-001` | Describe the unit, integration, or other test. | State the expected result that makes the test pass. | `path/to/test_file.rs` | `test_function_name` | ⏳ Not run / ✅ Success / ❌ Failed |

### Acceptance criteria

1. The specified public behavior is implemented and documented.
2. Relevant boundary and failure cases are tested.
3. Existing behavior and compatibility requirements are preserved.
4. The component passes the applicable formatting, lint, build, and test checks.

## Code coverage

Record the coverage results in this section or provide a path to a checked-in
coverage summary file. Include the coverage tool, the measured scope, the
coverage percentage, and any relevant threshold or uncovered areas.

- Maximize test coverage of the component's production code. Add tests for all
  reachable behavior and branches where practical; document and justify any
  remaining uncovered code. Do not reduce coverage by excluding relevant code
  from the measured scope.
- **Coverage summary:** Add results here, or link a summary file such as
  `path/to/coverage-summary.md`.

## Out of scope

- List explicitly excluded features, behaviors, and design decisions.
- Record deferred decisions and who or what must resolve them before implementation.
