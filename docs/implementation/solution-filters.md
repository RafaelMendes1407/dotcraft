# Solution filters

DotCraft loads JSON `.slnf` solution filters through the implementation in existing
[PR 2](https://github.com/RafaelMendes1407/dotcraft/pull/2), commit
`4cca4ff63aa65c28cebac79608592fd2422f2b8d`. The loader resolves the referenced `.sln` or
`.slnx` and displays only the selected projects in the solution explorer.

```json
{
  "solution": {
    "path": "../Example.slnx",
    "projects": ["src/Example.Api/Example.Api.csproj"]
  }
}
```

The solution path is relative to the filter file. Project paths are relative to the
referenced solution, with Windows path separators normalized on other hosts. Absolute
paths are supported according to the host platform. The filter remains the loaded
solution's path and supplies its display name, preserving it as the command target.

## Implemented scope

- Load selected projects from classic `.sln` and XML `.slnx` solutions.
- Retain selected projects in their original solution order and folder hierarchy.
- Remove folders emptied by filtering while preserving loose solution files.
- Avoid duplicating projects when the filter repeats an entry; accept an empty selection.
- Return errors for malformed JSON, missing required fields, absent source solutions,
  unsupported source extensions, and projects outside the referenced solution's membership.
- Render selected real projects in the native explorer without requiring an SDK or
  language-server process for the regression test.

The referenced source must be `.sln` or `.slnx`, preventing recursive filter loading.
Loading the solution/filter model reads files and does not itself execute project code.
Path normalization is lexical; files outside the filter directory are valid references.

## Validation

The existing source files were applied unchanged and tested independently on pristine
DotCraft commit `f30ac7180ce8dd492633235f3c3dbf2f4107e61b`:

```sh
cargo test -p rc-dotnet -p rc-core --lib --offline
cargo test -p rc-app solution_filter_explorer_shows_only_selected_real_projects --offline -- --nocapture
```

Results: 46 library tests and one headless explorer test passed. The library regressions
cover relative filter/source paths, Windows separators, `.slnx`, invalid inputs, duplicate
and empty selections, and real TenProjects fixture metadata. The explorer test displays
Module01 and Module10 and excludes Module02 and Module09 from that fixture.

## Remaining scope

This is the existing partial B-14 delivery. Actual SDK build/test execution against filters,
Roslyn handling, and watcher/search roots when the filter is outside the solution directory
still require validation. The explorer regression assigns the loaded model directly, so it
does not establish the complete application opening, indexing, and language-server lifecycle.
Cross-platform filesystem case and path behavior have not been exercised on Windows/macOS.

The source review and repeated tests did not require a production correction. This note adds
scope and validation evidence to the existing PR rather than introducing a duplicate feature.
