# Rust Component Requirements

- **Date:** YYYY-MM-DD
- **Author:** `{{GIT_USERNAME}}` (from `git config user.name`)
- **Revision:** 0.1
- **Short description:** A concise summary of the component and its purpose.

## Table of contents

- [Overview](#overview)
- [Revisions](#revisions)
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

## Revisions

Leave this table empty for the first draft. Each time the document revision
number changes, add one row with the new revision, the date, the author, and
the changes it introduces: requirements added, modified, or removed (by ID),
and other significant changes to the document. Keep rows in ascending
revision order.

| Revision | Date | Author | Changes |
|---|---|---|---|

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

The **Revision** column records the document revision in which the
requirement was last added or changed. Set it to the current document revision
when a requirement is created. Change it only when that requirement's content
is updated; leave it unchanged for requirements that were not modified, even
when the document revision increases. Record every requirement change in
[Revisions](#revisions). This applies to every requirements table below.

| ID | Requirement | Revision |
|---|---|---|
| `{{PROJECT_PREFIX}}-FR-001` | Describe an observable behavior the component must provide. | 0.1 |
| `{{PROJECT_PREFIX}}-FR-002` | Describe expected behavior for relevant inputs, boundary cases, and state changes. | 0.1 |

### Data and serialization

| ID | Requirement | Revision |
|---|---|---|
| `{{PROJECT_PREFIX}}-DS-001` | Specify data models, invariants, formats, and compatibility requirements, if applicable. | 0.1 |
| `{{PROJECT_PREFIX}}-DS-002` | Define how invalid, missing, or incomplete data is handled. | 0.1 |

### Rust-specific requirements

| ID | Requirement | Revision |
|---|---|---|
| `{{PROJECT_PREFIX}}-RS-001` | Specify required traits, ownership and borrowing behavior, lifetimes, and generic bounds. | 0.1 |
| `{{PROJECT_PREFIX}}-RS-002` | Define collection semantics and indexing behavior, if applicable. | 0.1 |
| `{{PROJECT_PREFIX}}-RS-003` | State the Rust edition, MSRV, and any relevant platform requirements. | 0.1 |

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

The number of project-specific phases is decided during this planning
reflection and differs from project to project. Whatever that number is, the
last two phases are always, in this order:

1. **Coverage**: run the tests with coverage, measure coverage for the files
   created or modified by this work only, and update
   [Code coverage](#code-coverage).
2. **Documentation**: update this requirements document (phases, checklist,
   test results, notes, revision) and the code comments (Rust doc comments on
   public items and inline comments) of the touched files.

Phase IDs use this scheme, where `N` is the number of project-specific phases:

- `P-{XXX}`: project-specific phases, numbered `P-001` to `P-{N}` following
  the usual ordering rules.
- `P-{N+1}`: Coverage, always the second-to-last phase.
- `P-{N+2}`: Documentation, always the last phase.

The two closing phases are the only exception to the ordering rules: they
always keep the last two IDs. If a project-specific phase is added later, it
takes the next `P-{XXX}` number, and the closing phases are renumbered to stay
`P-{N+1}` and `P-{N+2}`. For example, with three project-specific phases,
Coverage is `P-004` and Documentation is `P-005`.

| Phase ID | Phase and implementation steps | Dependencies | Completion criteria |
|---|---|---|---|
| P-{XXX} | Describe the project-specific phase and the implementation steps it includes. | List prerequisite phases or `None`. | State measurable or verifiable conditions for completing this phase. |
| P-{N+1} | **Coverage:** run the tests with coverage for the touched files only and record the results in [Code coverage](#code-coverage). | All project-specific phases. | Coverage is measured for every touched file, the results are recorded, and any uncovered code is justified. |
| P-{N+2} | **Documentation:** update this requirements document and the code comments of the touched files. | P-{N+1} | The document matches the implemented behavior and test results; public items in the touched files have up-to-date doc comments. |

### Current implementation checklist

Keep this checklist synchronized with the planned phases above. Add one item per
phase and update its state as work progresses. Check the box only when the phase
is complete; use the status label to distinguish phases that are not started,
in progress, or blocked. Record blockers in the item.

Phases are completed in order:

- A phase can be checked only if every phase before it in the checklist is
  already checked. The same rule applies to sub-tasks within a phase.
- When a new project-specific phase is added, it takes the next `P-{XXX}`
  number (after the last project-specific phase), and Coverage and
  Documentation are renumbered to stay last. If Coverage or Documentation
  were already checked, uncheck them and set them back to `Not started`,
  since they must be redone after the new phase. Record the reason in the
  implementation reflection.

- [ ] P-{XXX} — `{{PHASE_NAME}}` — Not started
- [ ] P-{N+1} — `Coverage of touched files` — Not started
- [ ] P-{N+2} — `Documentation update` — Not started

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
errors, boundaries, and compatibility. Add one row per test and link it to the
requirement IDs it verifies.

| Test ID | Requirement IDs | Description | Success requirements |
|---|---|---|---|
| T-001 | `{{PROJECT_PREFIX}}-FR-001` | Describe the unit, integration, or other test. | State the expected result that makes the test pass. |

#### Test location

Give the file and test function that implement each test listed above, using
one row per test ID.

| Test ID | File | Test name |
|---|---|---|
| T-001 | `path/to/test_file.rs` | `test_function_name` |

#### Test results

Record the result of each test. Each time a test is run, update its row with
the run date (`YYYY-MM-DD`), the short git hash of the commit tested
(`git rev-parse --short HEAD`, followed by `+dirty` if there were uncommitted
changes), and the status. Use one row per test ID. Status values:

- ✅ **Success**: the test passed.
- ❌ **Failed**: the test failed.
- ⏳ **Not run**: the test has not been executed yet. Use `—` for the date and hash.
- ⚠️ **Mitigate**: the test does not fully pass or cannot fully run, but the
  result has been analyzed and accepted for now. Every ⚠️ result must have an
  entry in [Warning notes](#warning-notes).

| Test ID | Date of run | Git hash | Status |
|---|---|---|---|
| T-001 | — | — | ⏳ Not run |

#### Notes

Record notes about test runs and results: the environment or command used,
flaky behavior, failures that were investigated and fixed, or anything else a
reviewer should know. Reference test IDs.

- None.

#### Warning notes

Add one entry for every test with ⚠️ **Mitigate** status. Explain why the test
is in that state, its impact on the requirements it verifies, and a possible
resolution or thoughts on how to resolve it, including who or what is needed.

- **T-xxx:** Why the status is ⚠️ Mitigate. Impact. Possible resolution or thoughts on how to resolve it.

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

### Current coverage results

Record the latest coverage results for the files created or modified by this
work, one row per file, sorted by file path. Give each value as a percentage
with covered/total counts (for example `95.00% (19/20)`), or `N/A` if the tool
does not report that metric. Replace the rows on each coverage run rather than
appending new ones.

- **Tool and command:** `{{COVERAGE_COMMAND}}`
- **Date of run:** YYYY-MM-DD
- **Git hash:** `{{GIT_SHORT_HASH}}`

| File name | Function coverage | Line coverage | Region coverage | Branch coverage |
|---|---|---|---|---|
| `path/to/file.rs` | 100.00% (0/0) | 100.00% (0/0) | 100.00% (0/0) | 100.00% (0/0) |

### Coverage notes

Add one entry for every file in the table above with any metric below 100%.
Identify the uncovered code (function, lines, or branches), explain why it
cannot be covered or is not covered yet, and describe any plan to cover it.
Write `None.` when every file is at 100%.

- **`path/to/file.rs`:** Uncovered code, why it is not covered, and the plan to cover it, if any.

## Out of scope

- List explicitly excluded features, behaviors, and design decisions.
- Record deferred decisions and who or what must resolve them before implementation.
