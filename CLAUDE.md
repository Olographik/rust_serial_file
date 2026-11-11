# Workspace instructions

This workspace is a collection of libraries shared with other projects. Each
top-level folder (for example `rust_serial_file/`, `rust_python_file/`,
`serialized_struct_derive/`, `utils/`) is an independent module. Keep public
APIs stable and documented, since downstream projects depend on them.

## Plans and design reflections

Whenever you create a plan, a design reflection, or the requirements for a new
feature or component (including when in plan mode), you must use the
`requirements-plan` skill:

- Follow the template in [skills/template/requirements.md](skills/template/requirements.md)
  exactly: keep every section, its order, and the table formats.
- Write the result to
  `{module_name}/doc/requirements/{submodule(s)}/{short_requirement_name}.md`,
  where `{module_name}` is the module folder the work belongs to,
  `{submodule(s)}` mirrors the Rust submodule path under `src/` (nested
  folders for nested submodules, for example `file` for `src/file/`), and
  `{short_requirement_name}` is a short `snake_case` name for the requirement
  (for example `rust_serial_file/doc/requirements/file/struct_file.md`).
- If the work spans several modules, write one document per module, or ask
  which module owns it.
- Update an existing requirements document instead of creating a duplicate,
  and bump its revision.
- Do not start implementing until the plan document has been written and the
  user has approved it.
