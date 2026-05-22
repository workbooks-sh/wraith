def prefix: $ENV.PREFIX // "";
def pm: $ENV.PKG_MANAGER // "npm";
def remove(pkg): if pm == "pnpm" then "pnpm remove \(pkg)" elif pm == "yarn" then "yarn remove \(pkg)" else "npm uninstall \(pkg)" end;
def add(pkg): if pm == "pnpm" then "pnpm add \(pkg)" elif pm == "yarn" then "yarn add \(pkg)" else "npm install \(pkg)" end;
def add_dev(pkg): if pm == "pnpm" then "pnpm add -D \(pkg)" elif pm == "yarn" then "yarn add -D \(pkg)" else "npm install --save-dev \(pkg)" end;
def footer(rule): "\n\n---\n<sub><a href=\"https://docs.fallow.tools/explanations/dead-code#" + rule + "\">Docs</a> \u00b7 Disagree? <a href=\"https://docs.fallow.tools/configuration/suppression\">Configure or suppress</a></sub>";
def workspace_context:
  if ((.used_in_workspaces // []) | length) > 0 then
    "\n\nThis package is imported in another workspace: " + (.used_in_workspaces | map("`\(.)`") | join(", ")) + ". Consider moving the dependency to the consuming workspace's `package.json`."
  else
    ""
  end;
def dependency_action(pkg):
  if ((.used_in_workspaces // []) | length) > 0 then
    "\n\n**Action:** Move this dependency to the workspace that imports it."
  else
    "\n\n**Action:** If nothing in your code imports this package, remove it:\n\n```sh\n\(remove(pkg))\n```"
  end;
[
  (.unused_files[]? | {
    type: "unused-file",
    path: (prefix + .path),
    line: 1,
    body: ":warning: **Unused file**\n\nThis file is not imported by any module and is unreachable from all entry points.\n\n<details>\n<summary>Why this matters</summary>\n\nUnused files mislead developers into thinking this code is still active \u2014 leading to wasted time reading, maintaining, and reviewing dead code. They also slow down IDE indexing and search results.\n</details>\n\n**Action:** Delete this file, or import it where needed.\(footer("unused-files"))"
  }),
  (.unused_exports[]? | {
    type: "unused-export",
    export_name: .export_name,
    path: (prefix + .path),
    line: .line,
    body: ":warning: **Unused \(if .is_type_only then "type " else "" end)export**\n\n\(if .is_re_export then "Re-exported" else "Exported" end) \(if .is_type_only then "type" else "value" end) `\(.export_name)` is never imported by other modules.\n\n<details>\n<summary>Why this matters</summary>\n\nUnused exports signal to other developers that this code is used elsewhere \u2014 so nobody touches it, even when it should change. They also prevent bundlers from tree-shaking this code out of production.\n</details>\n\n**Action:** Remove the `export` keyword, or delete the declaration entirely.\n\n> Intentionally public? Add a `/** @public */` JSDoc tag above the export to tell fallow it\u2019s part of your API.\(footer("unused-exports"))"
  }),
  (.unused_types[]? | {
    type: "other",
    path: (prefix + .path),
    line: .line,
    body: ":warning: **Unused type export**\n\n\(if .is_re_export then "Re-exported" else "Exported" end) type `\(.export_name)` is never imported by other modules.\n\n**Action:** Remove the `export` keyword if only used internally.\(footer("unused-types"))"
  }),
  (.private_type_leaks[]? | {
    type: "other",
    path: (prefix + .path),
    line: .line,
    body: ":warning: **Private type leak**\n\nExport `\(.export_name)` references same-file private type `\(.type_name)` in its public signature.\n\n**Action:** Export `\(.type_name)` or remove it from the public signature.\(footer("private-type-leaks"))"
  }),
  (.unused_dependencies[]? | {
    type: "other",
    path: (prefix + .path),
    line: (if .line > 0 then .line else 1 end),
    body: ":warning: **Unused dependency**\n\n`\(.package_name)` is listed in `\(.location)` but no file imports it directly.\(workspace_context)\(dependency_action(.package_name))\n\n<details>\n<summary>Why this matters</summary>\n\nUnused dependencies slow down installs, inflate `node_modules`, and add noise to security audits.\n</details>\n\n> Some packages are used indirectly (peer dependencies, framework internals, or plugin systems). If that\u2019s the case, add it to [`ignoreDependencies`](https://docs.fallow.tools/configuration/overview) in `.fallowrc.json`.\(footer("unused-dependencies"))"
  }),
  (.unused_dev_dependencies[]? | {
    type: "other",
    path: (prefix + .path),
    line: (if .line > 0 then .line else 1 end),
    body: ":warning: **Unused devDependency**\n\n`\(.package_name)` is listed in `devDependencies` but no file imports it in this workspace.\(workspace_context)\(dependency_action(.package_name))\n\n> Used by a tool that doesn\u2019t import directly (e.g., a CLI, plugin, or preset)? Add it to [`ignoreDependencies`](https://docs.fallow.tools/configuration/overview) in `.fallowrc.json`.\(footer("unused-dependencies"))"
  }),
  (.unused_optional_dependencies[]? | {
    type: "other",
    path: (prefix + .path),
    line: (if .line > 0 then .line else 1 end),
    body: ":warning: **Unused optionalDependency**\n\n`\(.package_name)` is listed in `optionalDependencies` but no file imports it in this workspace.\(workspace_context)\(dependency_action(.package_name))\(footer("unused-dependencies"))"
  }),
  (.unused_enum_members[]? | {
    type: "other",
    path: (prefix + .path),
    line: .line,
    body: ":warning: **Unused enum member**\n\n`\(.parent_name).\(.member_name)` is never referenced anywhere in the codebase.\n\n**Action:** Remove this member to keep the enum minimal.\n\n> Run `fallow fix` to auto-remove unused enum members.\(footer("unused-enum-members"))"
  }),
  (.unused_class_members[]? | {
    type: "other",
    path: (prefix + .path),
    line: .line,
    body: ":warning: **Unused class member**\n\n`\(.parent_name).\(.member_name)` is never referenced.\n\n**Action:** Remove it or restrict its visibility.\(footer("unused-class-members"))"
  }),
  (.unresolved_imports[]? | {
    type: "other",
    path: (prefix + .path),
    line: .line,
    body: ":x: **Unresolved import**\n\nImport `\(.specifier)` could not be resolved to a file or package.\n\n**Check for:**\n- Typo in the import path\n- File exists but isn\u2019t included in `tsconfig.json` (`include`/`exclude`)\n- Missing dependency \u2014 run `\(add("<package>"))`\n- Path alias mismatch in `tsconfig.json` `paths`\(footer("unresolved-imports"))"
  }),
  (.unlisted_dependencies[]? | (.package_name) as $pkg | .imported_from[]? | {
    type: "other",
    path: (prefix + .path),
    line: .line,
    body: ":x: **Unlisted dependency**\n\n`\($pkg)` is imported here but not declared in `package.json`. This will fail on a clean install.\n\n```sh\n\(add($pkg))\n```\(footer("unlisted-dependencies"))"
  }),
  (.duplicate_exports[]? | .locations as $locs | .locations[0] as $loc | {
    type: "other",
    path: (prefix + $loc.path),
    line: $loc.line,
    body: ":warning: **Duplicate export**\n\nExport `\(.export_name)` is defined in \($locs | length) modules:\n\n\($locs | map("- `\(.path):\(.line)`") | join("\n"))\n\nThis causes ambiguity \u2014 barrel files may re-export the wrong one.\n\n**Action:** Keep one canonical location and remove the others.\(footer("duplicate-exports"))"
  }),
  (.circular_dependencies[]? | {
    type: "other",
    path: (prefix + .files[0]),
    line: (if .line > 0 then .line else 1 end),
    body: ":warning: **Circular dependency**\n\nCircular import chain detected:\n\n```\n\(.files | join(" \u2192 ")) \u2192 \(.files[0])\n```\n\n<details>\n<summary>Why this matters</summary>\n\nCircular dependencies can cause:\n- **Runtime crashes** \u2014 modules may be `undefined` because they haven\u2019t finished initializing when first imported\n- **Import-order bugs** \u2014 behavior changes depending on which file is loaded first\n- **Broken code splitting** \u2014 bundlers may merge circular modules into a single chunk, defeating lazy loading\n</details>\n\n**Action:** Extract shared logic into a separate module that both files can import.\(footer("circular-dependencies"))"
  }),
  (.boundary_violations[]? | {
    type: "other",
    path: (prefix + .from_path),
    line: (if .line > 0 then .line else 1 end),
    body: ":no_entry_sign: **Boundary violation**\n\nImport from zone `\(.from_zone)` to zone `\(.to_zone)` is not allowed:\n\n`\(.from_path)` \u2192 `\(.to_path)`\n\n<details>\n<summary>Why this matters</summary>\n\nArchitecture boundaries enforce separation of concerns. Crossing them can:\n- **Create hidden coupling** \u2014 changes in one layer break another\n- **Defeat modularity** \u2014 zones become entangled and hard to refactor independently\n</details>\n\n**Action:** Route the import through an allowed zone, or restructure the dependency.\(footer("boundary-violations"))"
  }),
  (.type_only_dependencies[]? | {
    type: "other",
    path: (prefix + .path),
    line: (if .line > 0 then .line else 1 end),
    body: ":blue_book: **Type-only dependency**\n\n`\(.package_name)` is only used in `import type` statements \u2014 it\u2019s not needed at runtime.\n\n**Action:** Move it to `devDependencies`:\n\n```sh\n\(add_dev(.package_name)) && \(remove(.package_name))\n```\n\n> Publishing a library? If consumers need these types, keep it in `dependencies`.\(footer("type-only-dependencies"))"
  }),
  (.stale_suppressions[]? | {
    type: "other",
    path: (prefix + .path),
    line: .line,
    body: (if .origin.type == "jsdoc_tag" then
      ":broom: **Stale @expected-unused**\n\nThe `@expected-unused` tag on `\(.origin.export_name)` is stale because the export is now used.\n\n**Action:** Remove the `@expected-unused` tag.\(footer("stale-suppressions"))"
    else
      ":broom: **Stale suppression**\n\nThis `\(if .origin.is_file_level then "fallow-ignore-file" else "fallow-ignore-next-line" end)` comment\(if .origin.issue_kind then " for `\(.origin.issue_kind)`" else "" end) no longer matches any active issue.\n\n**Action:** Remove the suppression comment to keep the codebase clean.\(footer("stale-suppressions"))"
    end)
  })
] | .[:($ENV.MAX | tonumber)]
