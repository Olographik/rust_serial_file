---
name: requirements-plan
description: Write a plan, design reflection, or requirements document for a module of this library workspace, using the shared requirements template. Use whenever creating or updating a plan, a design, a feature specification, or a phase breakdown, including in plan mode, before any implementation starts.
---

# Requirements plan

Use this procedure for every plan or design reflection in this workspace.

## 1. Identify the target

1. Find the **module**: the top-level workspace folder that owns the work
   (for example `rust_serial_file`). If it is ambiguous or the work spans
   several modules, ask the user, or write one document per module.
2. Find the **submodule(s)**: the Rust submodule path under the module's
   `src/` that the work belongs to (for example `file` for `src/file/`,
   `data_container`, `serialized_struct`). Use one folder per nesting level for
   nested submodules (for example `file/reader`). Reuse an existing folder in
   `{module_name}/doc/requirements/` when one already matches.
3. Choose a **short requirement name** in `snake_case` (for example
   `struct_file`, `buffered_writer`).
4. The output path is:

   ```
   {module_name}/doc/requirements/{submodule(s)}/{short_requirement_name}.md
   ```

   For example `rust_serial_file/doc/requirements/file/struct_file.md`.
5. Look for an existing document on the same topic in
   `{module_name}/doc/requirements/` (including subfolders). If one exists,
   update it instead of creating a new file: increase the **Revision** and
   update the **Date**.

## 2. Gather context

- Read [the template](../../../skills/template/requirements.md) in full.
- Read the module's relevant source, tests, and `Cargo.toml`, plus related
  requirement documents, so that the requirements, public API, and
  compatibility sections reflect the real code.
- Get the author with `git config user.name`.

## 3. Write the document

Fill in the template, keeping every section, the heading order, the table of
contents, and the table columns:

- **Header:** today's date (`YYYY-MM-DD`), the git user name as author,
  revision `0.1` for a new document, and a one-sentence short description.
- **Requirement IDs:** choose one short uppercase project prefix for the
  module (reuse the prefix already used in that module's documents) and number
  IDs as `{PREFIX}-FR-001`, `{PREFIX}-NFR-001`, and so on. Never renumber
  existing IDs.
- **Requirement revision column:** set each new requirement's **Revision** to
  the current document revision. Change a requirement's Revision only when
  that requirement's content is added or changed; never bump it for
  unchanged requirements just because the document revision increased.
- **Revisions section:** leave the Revisions table empty for the first draft
  (`0.1`). Whenever the document revision number changes, add one row (new
  revision, date, author, and the changes: requirement IDs added, modified, or
  removed, and other significant document changes), in ascending revision
  order. Every requirement whose Revision cell changed must appear in that
  row.
- **ID ordering (applies to every ID: requirements, phases `P-xxx`, tests
  `T-xxx`):**
  - Number IDs sequentially within each category, starting at `001`, with no
    gaps or duplicates.
  - Keep table rows and checklist items sorted in ascending ID order.
  - A new item always takes the next number after the highest existing ID in
    its category and is placed at the end of its table. Do not insert a new
    ID between existing ones.
  - Before finishing, check every table and the checklist for ordering, gaps,
    and duplicates.
- **Phase sub-tasks:** when a phase is split into sub-tasks, give each
  sub-task the parent phase ID followed by an uppercase letter in alphabetical
  order, starting at `A` (`P-001-A`, `P-001-B`, ...). The same ordering rules
  apply: no gaps, no duplicates, and a new sub-task takes the next letter.
  List sub-tasks directly under their parent phase, both in the phase table
  and in the checklist, for example:

  ```markdown
  - [ ] P-001 — `Phase name` — In progress
    - [x] P-001-A — `Sub-task name` — Done
    - [ ] P-001-B — `Sub-task name` — Not started
  ```

  A parent phase is complete only when all its sub-tasks are complete.
- **Sequential completion:** a phase (or sub-task) can be checked only if
  every phase (or sub-task) before it in the checklist is already checked.
  Never check a phase out of order.
- **Adding a phase:** a new project-specific phase takes the next `P-{XXX}`
  number after the last project-specific phase, and Coverage and
  Documentation are renumbered to stay `P-{N+1}` and `P-{N+2}`. If Coverage or
  Documentation were already checked, uncheck them and set them back to
  `Not started`. They must be redone after the new phase. Record the reason
  in the implementation reflection.
- **Phases:** design the phases for this specific work (dependencies,
  completion criteria) and keep the implementation checklist in sync, one item
  per phase, all `Not started` initially. Answer the planning reflection
  questions. The number of project-specific phases is decided during the
  reflection; do not use a fixed count. The last two phases are always, in
  this order. With `N` project-specific phases (`P-001` to `P-{N}`), the IDs
  are:
  - `P-{XXX}`: project-specific phases, following the ordering rules.
  - `P-{N+1}` — **Coverage:** run the tests with coverage for the files
    created or modified by this work only, and update the Code coverage
    section: replace the rows of the **Current coverage results** table (file
    name, function, line, region, and branch coverage) along with the tool,
    date, and git hash. For every file with a metric below 100%, add an entry
    in **Coverage notes** that identifies the uncovered code, explains why,
    and gives any plan to cover it.
  - `P-{N+2}` — **Documentation:** update the requirements document and the
    code comments (doc comments and inline comments) of the touched files.

  In the written document, replace these placeholders with real numbers (for
  example, with 3 project phases: `P-004` Coverage and `P-005` Documentation).
  These two closing phases are the only exception to the ordering rules: if a
  project-specific phase is added later, it takes the next `P-{XXX}` number,
  and the closing phases are renumbered to stay `P-{N+1}` and `P-{N+2}`.
  Update every reference to them.
- **Tests:** list every planned test, linked to requirement IDs, in the tests
  table. Give its intended file and test name in **Test location**, and add it
  to **Test results** with status `⏳ Not run`. All three tables must contain
  the same test IDs, in the same order.
- Replace every `{{PLACEHOLDER}}` and instruction text with real content. If a
  section does not apply, say so explicitly (for example "None: no `unsafe`
  code is allowed.") rather than deleting it.
- Put open questions and deferred decisions in **Out of scope**, stating who
  must resolve them.

## 4. Review and hand over

- Show the user the path of the document and summarize the key decisions and
  open questions.
- Do not start implementation until the user approves the plan.

## 5. During implementation

Keep the document current as work progresses: update the phase checklist,
the implementation reflection, and the code coverage section. After each test
run, update **Test results** with the date, the short git hash
(`git rev-parse --short HEAD`, plus `+dirty` if there were uncommitted changes),
and the status (`✅ Success`, `❌ Failed`, `⏳ Not run`, `⚠️ Mitigate`). Add
relevant observations to **Notes**. Every `⚠️ Mitigate` result needs an entry
in **Warning notes** explaining why, and a possible resolution.
