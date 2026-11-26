# PHP engine architecture

A guide for contributors to the PHP engine, `crates/phoxite`. This note says
where things are. The user-facing side is in the [README](../../README.md)
and [docs/editors.md](../editors.md).

The engine is clean-room. Its design inputs were public documentation (the
IntelliJ Platform SDK pages on PSI stubs, indexes and type providers, PhpStorm's
advanced metadata, the PHPStan and Psalm type docs) and open-source code with
compatible licences. Don't read or copy closed-source products.

## The request path

```
editor ── LSP (stdio) ──▶ phoxite-lsp server.rs ──▶ handlers.rs ──▶ PhpEngine
```

- `crates/phoxite-lsp/src/server.rs` is the server loop: initialization, the
  client's notifications (handled in order on its thread), requests (each on
  its own thread, cancellable) and the engine's events (diagnostics, indexing
  progress).
- `state.rs` holds one `PhpEngine` per workspace folder, the open documents,
  the settings and the problems published per file. `config.rs` maps the
  client's settings onto the engine's `PhpConfig`.
- `handlers.rs` answers the standard requests and `convert.rs` turns the
  engine's types (`phoxite::base`) into the LSP's.
- Twig templates (`*.twig`, whatever language id the client sends) need no
  engine: `phoxite::twig` answers the outline, completion, hover, goto, the
  usages of a template name, rename, formatting, semantic tokens and the
  syntax errors from the document's text
  and the project's template directories. `state.rs` keeps what was read
  about a project (`twig_project`) until a template, `twig.yaml` or a
  Composer file changes there.
- Blade templates (`*.blade.php`, whatever language id the client sends):
  directive completion and view/component goto need no engine. For the PHP
  inside echoes, directives and `@php` blocks the server asks the engine:
  `phoxite::blade::compile` turns the view into PHP with an offset map,
  `PhpEngine::embedded_document` holds it under the view's id (never
  inspected, ignored by `document_changed`), and the `blade_*` methods map
  positions in and answers out.
- `analyzers.rs` runs PHPStan and Psalm from the project's `vendor/bin` on
  each saved PHP file when `analyzers.onSave` is on, and publishes their
  problems under the sources `phpstan` and `psalm`. `phoxite::analyzers`
  detects the tools, parses their JSON reports and drops the problems the
  engine's latest inspection of the file already reports.
- `laravel.rs` (`phoxite/laravel/*`) and `refactor.rs` (`phoxite/refactor/*`)
  hold the requests beyond standard LSP: the routes list, the model view, the
  generators, Artisan, and the refactorings that ask the user first.

Coordinates at the crate edge are 0-based lines and UTF-16 columns
(`phoxite::base`). Mago works in byte offsets; `syntax::LineIndex` converts.

## Layers

From the bottom up. Each module's `//!` header is the detailed reference.

| Module                                          | What it holds                                                                                                                                                                                                                                                     |
| ----------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `syntax`                                        | `ParsedFile` (Mago AST, errors, `LineIndex`) and the parse cache keyed by `(DocId, version)`. Mago is pinned and re-exported as `phoxite::mago`.                                                                                                                  |
| `names`                                         | PHP name resolution: namespaces, `use` (function, const, grouped), aliases, the global fallback for functions and constants, `self`/`static`/`parent` markers.                                                                                                    |
| `summary`                                       | `FileSummary`: what other files can see of one file (class-likes with members and signatures, functions, constants, PHPDoc as unresolved type expressions, ranges). Serialized with postcard and stamped with `SCHEMA_VERSION`; golden snapshots in `snapshots/`. |
| `project`                                       | `ProjectLayout`: which files belong to the project (the folder minus excludes, Composer autoload and `installed.json`, include paths) and the PSR-4/PSR-0 `AutoloadMap`.                                                                                          |
| `index`                                         | `PhpIndex`: one redb database in the project cache folder. Summaries by content hash, FQN, short name, direct subclasses, namespace children, `.phpstorm.meta.php` files and a word index for find usages. Built in parallel, updated per changed file.           |
| `stubs`                                         | `BundledStubs`: phpstorm-stubs, checked in as `stubs/phpstorm-stubs.zip` and embedded; summarised once per process and filtered per project by PHP version and stub sets (`StubFilter`).                                                                          |
| `engine`                                        | `PhpEngine`: the worker thread that keeps the index up to date, dumb-mode and readiness events, open documents as overlays on the disk, and the modification counter.                                                                                             |
| `types`                                         | The `Type` model and PHPDoc parsing, `LazyType` and the `Resolver` (with its cache), `ClassModel` (own and inherited members), expression types and the per-function flow graph (`types/expr/flow.rs`), and `TypeProvider`s.                                      |
| `meta`                                          | `.phpstorm.meta.php`: return-type overrides and exit points (through a type provider), expected arguments for completion.                                                                                                                                         |
| `complete`, `signature`, `semantic`, `navigate` | Completion; signature help and inlay hints; semantic tokens; symbols, definition, hover, references, hierarchies, rename and file moves.                                                                                                                          |
| `imports`, `format`                             | Auto-import, Optimize Imports, `use` insertion; Reformat Code through `mago-formatter`.                                                                                                                                                                           |
| `inspect`                                       | The `Inspection` trait, the built-in checks, `@noinspection`, quick-fixes and intentions, the debounced background `runner` that publishes `IdeEvent::Diagnostics { source: "php" }`, and `inspect_file` for Inspect Code.                                        |
| `refactor`                                      | Extract variable/constant/method, inline, change signature, pull up/push down, generate code. Edits are `WorkspaceEdit`s (`phoxite::base`).                                                                                                                         |

### Types in two phases

At index time a summary can't look at other files, so any type that depends on
one is stored as a `LazyType` (`FnReturn("\foo")`, `MethodReturn { recv, name }`,
an unannotated return inferred from the body, …). At request time the `Resolver`
completes it against the indexes, member by member for unions, with `visited`
and `depth` guards against cycles. Resolved types and class models are cached
until the engine's modification counter moves: every document edit and watcher
batch bumps it.

Variable types come from the flow graph of the enclosing function: assignments,
narrowing (`instanceof`, `is_*`, null and `false` checks, `assert`, `match
(true)`, `@phpstan-assert`), inline `@var`, by-reference arguments, and closure
parameters typed from the callable they're passed to. The same `ExprTypes` is
shared by every inspection of one run.

### Type providers

`types::provider::TypeProvider` lets a framework add types without touching the
core: a key, local steps for a call site or an untyped property (`call`,
`property`, which return a `LazyType::Provided` payload) and a resolve step
(`resolve`, with the indexes). PHPUnit test doubles, `setUp()` fields,
`.phpstorm.meta.php`, Laravel and Symfony are built in. Register a new provider
in `TypeProviders::default`.

What a string names (a route, a view, a configuration key) is not a type but a
string reference. Those, and how the engine knows which frameworks a project
uses, are in [frameworks.md](frameworks.md).

### Pest

`$this` in the closures of a Pest test file (`test()`, `it()`, `todo()`,
`beforeEach()`, `afterEach()`, and the closures of `describe()` blocks, through
chained calls) is the test case Pest binds them to, with that class's member
access: the first class (not trait) of the file's own `uses(…)`, else of the
`uses(…)->in(…)` or `pest()->extend(…)->use(…)->in(…)` whose folder (relative
to the configuring file, usually `tests/Pest.php`) holds the file most
closely, else PHPUnit's `TestCase`. The calls are indexed as Laravel facts
(`PestUses`); `ExprTypes::at_path` gives the analysis the file's path, which
the editor features, Inspect Code and Find Usages pass.

Custom expectations (`expect()->extend('toBeX', function (…) { … })`) are
indexed as macros of `Pest\Expectation` returning the expectation, and `$this`
in their closure is the expectation. A property of `$this` the test case
doesn't declare has the type the file's `beforeEach()` closures assign to it
(`$this->manager = app(Manager::class);`, also inside `describe()` blocks).
Higher-order expectations work through the PHPUnit provider: a method of the
expected value called on the expectation (`expect($order)->canRefund()`)
resolves to the value's method and stays an expectation, and so does a
property of the value (`->state->toBe(…)`, without the value's type from
then on). `test()` without arguments is the test case of the file
(`test()->createProduct()` in a helper function). Not done: members the
`uses()` traits add beyond the test case class.

### What nothing declares

These rules type code that carries no declaration, each from the code around
it. The first three are inference every feature uses, inspections included;
the last three are guesses only what the editor shows uses
(`ExprTypes::with_call_sites`):

- **An untyped property** (no hint, no `@var`) has the union of what the
  methods of its class assign to it, when that is at most four different
  things and none of them unknown (`CoreProvider::property`; a test case's
  `setUp()` assignments come first, through the PHPUnit provider). The type
  is stored in the summary as the property's default type. Elements count
  too: `$this->items[] = $item;` and `$this->byName[$key] = $item;` make it a
  list (an array) of what is added, and refine a property declared no more
  than `array` (the hint, or `@var array`); a documented element type wins.
  A property declared `mixed` counts as untyped. A property a class
  inherits untyped and assigns itself has, in that class and below, what
  that class assigns (`ClassLike::refined`, read by
  `Resolver::refined_property`); the parent's declaration is unchanged.
- **A method declared a bare container** (`: array`, or a class whose name
  ends in `Collection`) without a `@return` returns what its body does, when
  that is the same container with its element types
  (`Resolver::refines_container`). The extractor stores the body's type for
  these as it does for a method without a return type.
- **An anonymous class** is a class-like the summary holds apart from the
  named ones (`FileSummary::anonymous`), under a name nothing can write
  (`summary::anonymous_fqn`: a tag of the file's text and the offset). `new
  class { … }` has that type, so its members, its traits' and its parent's
  work on the object like a named class's; a function or method declared
  `: object` that returns one returns it. The index finds these classes by
  that name only: they are in no short-name, namespace or subclass table,
  so no completion, search or hierarchy lists them. Types show them as
  `class@anonymous`. Inside the class's own methods `$this` is still
  handled from the syntax tree (the frame of an anonymous class has no
  name), as described below.
- **`$this` in a trait** is also an object of the classes using the trait
  (`Resolver::trait_hosts`: up to three, in name order), as an intersection,
  so what the trait expects of its host resolves. The index's subclass table
  records trait uses for this.
- **`$this` in an anonymous class**, which no summary holds, reaches the
  members the class declares (read from the syntax tree, in the file) and
  those of the class it extends, for Go to Declaration and hover; not for
  completion, Find Usages or Rename.
- **An untyped parameter** (no hint, no `@param`, nothing from the method it
  overrides or from a provider) takes the types its callers pass, when
  the analysis asks for it: `ExprTypes::with_call_sites`, which completion,
  `at_caret` (hover, Go to Declaration, the subject of Find Usages) and
  signature help turn on. **Inspections, semantic tokens, inlay hints and
  the references scan never do:** the type is a guess (a caller may be
  missing or wrong), and the whole-file passes would read other files on
  every edit. The callers are the calls of the method on `$this`, `self::`,
  `static::`, `parent::` or a receiver typed with the class or a subclass
  (`types::expr::call_site_arguments`, which finds them by the method's name
  in the text and types them with the file's own `ExprTypes`): in the file
  itself; for a method that isn't private, in the files of the class and
  its subclasses; for a public one, also in the indexed files containing the
  method's name when they are few (`Declarations::call_site_types`, at most
  16 files in all, cached per modification count). Unknown arguments are
  left out; one to four different types make the union, more make nothing;
  a caller whose argument is itself an untyped parameter is followed three
  levels deep. Hover adds where the type comes from. A parameter declared
  `object` is refined the same way, and one declared a bare container when
  every caller passes that container with its elements. Not done: free
  functions, and methods called from more files than the cap.
- **A method call nothing types** (no return type, no `@return`, and the
  index-time inference of the body gave up, often because the result
  depends on an untyped parameter) is typed from the body with the full
  analysis: `ExprTypes::body_return`, in the file itself or, through
  `Declarations::body_return`, in the file declaring the class (parsed on
  demand, cached per modification count). A call on a receiver already
  found unknown isn't asked again, which keeps long chains linear. Free
  functions are covered the same way, for returns and for parameters.
- **A member only subclasses declare:** when the receiver's class has no
  such method or property, its subclasses and implementors are looked at
  (`Resolver::subtype_members`: transitive, at most 32 classes, not below a
  `final` class). Go to Declaration offers every class declaring it, and
  the call has the union of what they return (at most four types).

### Class aliases

`class_alias('Real', 'Alias')` is a name fact (`DeclaredKind::ClassAlias`,
the alias as the name and the class as its one class). The index lists the
files that have some under their own key (`CLASS_ALIAS_FILES_KEY`): every
class lookup that finds nothing asks, and must not read every file with name
facts. `PhpEngine::class_aliases` maps each alias to its class (the project's
own alias over a library's) until something is edited, and
`PhpEngine::class_alias` follows an alias of an alias (at most eight, `None`
on a loop). A namespaced `Foo\class_alias()` is another function. A declared
class wins: `class_like`, `declares` and `declarations_of` ask for the alias
only when the project and the stubs have no class of that name, and then
answer with the class it stands for. Only names written as strings or
`Foo::class` count.

## Threads and cancellation

- Indexing runs on the engine's worker thread and rayon. It can be cancelled
  and reports `IdeEvent::IndexState` while it runs. The engine also reports
  itself as a language server (`starting` → `busy` → `ready`), for a host that
  treats it like one.
- Requests run on the host's threads (`phoxite-lsp` gives each its own), and
  take a `CancellationToken` where the work is large (references, rename,
  workspace symbols, implementations).
- Background inspections wait 120 ms after the last change to a document; a
  newer change cancels the run in progress.

## Adding to the engine

- **An inspection:** implement `inspect::Inspection` in the module of its family,
  and register it in `Inspections::builtin`. Write unit tests with an inline fixture and a `/*|*/`
  caret, plus a before/after test for each quick-fix.
- **A feature of an existing request:** change the engine module, then the
  conversion in `crates/phoxite-lsp/src/convert.rs` if the result shape changed.
- **A new request:** add its handler to `crates/phoxite-lsp/src/handlers.rs`
  (or `laravel.rs` / `refactor.rs` for a `phoxite/*` request), route it in
  `server.rs` and advertise it in the capabilities.
- **A summary field:** bump `summary::SCHEMA_VERSION` (old indexes are then
  rebuilt) and update the golden snapshots.

## Testing and measuring

- Unit tests sit next to each module (`tests.rs`, `*_tests.rs`):
  `cargo nextest run -p phoxite` or `cargo test -p phoxite`.
- `crates/phoxite-lsp/src/tests.rs` runs the server end to end over an
  in-memory connection, on the fixtures.
- The corpus tests (`crates/phoxite/tests/`, ignored) run on pinned releases of
  laravel/framework, symfony and WordPress, which `scripts/php-corpus.sh` clones
  into `../php-corpus`; `scripts/laravel-apps.sh` adds two real Laravel
  applications with their `vendor/` for `LARAVEL_APP_DIR`.
- `crates/phoxite/tests/differential.rs` (ignored; needs Intelephense and, for
  `corpus`, the corpora) compares definition and hover with Intelephense on
  sampled positions. The last report is in [differential.md](differential.md).
- `crates/phoxite/benches/engine.rs` holds the performance budgets
  (`cargo bench -p phoxite --bench engine`); see
  [performance.md](performance.md). `tests/memory.rs` compares memory with
  Intelephense.
- `examples/parse_corpus.rs` measures the parser on the corpora
  ([spike.md](spike.md)).
