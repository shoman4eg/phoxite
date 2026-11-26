# Phoxite

PHP, Laravel and Twig code intelligence written in Rust, packaged for any editor: a library
(`phoxite`), a language server (`phoxite-lsp`) and a VS Code extension.

Phoxite is a standalone engine: it depends on no editor.
[OpenIDEA](https://github.com/l0ng-ai/openidea) offers it as one of its PHP language
servers. What it does:

- **PHP:** completion with auto-import, hover, signature help, go to definition, type
  definition and implementation, Find Usages, rename (files named after a class move with
  it), document highlights, inlay hints, semantic highlighting, call and type hierarchies,
  quick-fixes and intentions, Optimize Imports, Format Document (PSR-12 via Mago), and
  inspections with per-inspection settings. PHP up to 8.6, partial function application
  included; syntax newer than the project's version is reported. An alias `class_alias()` makes
  is the class it stands for. Moving PHP files updates their namespaces and
  every reference. Change Signature, Extract Method with its options, the Generate member
  choosers and Pull Members Up / Push Members Down ask the user first: the VS Code
  extension has their dialogs, and `docs/editors.md` says what another client needs.
  PHPStan and Psalm can run on save when the project installs them.
- **Laravel:** routes, views and Blade components, config, env, translations, validation
  rules, Eloquent (columns from migrations, relations, scopes, factories), gates and
  policies, Artisan commands, Livewire, Inertia, macros and modules; route code lenses and
  URI hints; Laravel inspections with quick-fixes. The server also answers what an editor
  needs for a routes list, the Eloquent model view, the code generators, running Artisan
  commands and "Create translation key…" (`docs/editors.md`); the VS Code extension has
  them all.
- **Blade:** completion, hover, goto and usages in echoes, directives and `@php` blocks;
  directive, section, stack and component names; the template's own problems; formatting.
- **Twig:** the outline; completion of tags, filters, functions, tests, the variables and
  macros in scope and template names; hover; goto for template names, macros and the block
  a block overrides; the usages of a template name; rename of blocks and macros; formatting
  (indentation and the spacing inside tags); syntax errors. With Symfony's Twig bridge installed, its
  tags, filters and functions too.
- **Symfony:** the types of services (`$container->get()`), of Doctrine repositories, their
  finders and the entities' associations; route names and Twig template names with
  completion, goto, usages and rename, in PHP and in the templates; service ids,
  parameters, translation keys and the fields in a repository's criteria. Templates know
  the variables their controllers pass them and the project's own Twig filters.
- **WordPress:** hooks, shortcodes, post types, taxonomies, script and style handles,
  options, image sizes and menu locations with completion, goto, usages and rename;
  template parts; the types of a hook callback's parameters and of `global $wpdb`.
- **CodeIgniter** (4, and 3): views, helpers, language lines, routes, filters, view cells,
  events, validation rules, tables and their columns, configurations, models and services
  by name, with a warning for one that names nothing; the types of `service()`,
  `model()` and `config()`, of what a model finds, and of the variables a controller
  passes its view. In CodeIgniter 3, what `$this->load` loads and gives the controller.
- **Yii 2:** views, layouts, routes, components, modules, parameters, path aliases,
  messages and model attributes by name, with a warning for a view, a route or an
  attribute that names nothing; the types of `Yii::$app`'s components, of
  `Yii::createObject()`, of Active Record queries and relations, of a record's columns
  (from the migrations), of properties read through a getter, and of the variables a
  controller passes its view. Views are found where a `viewPath` or a theme puts them.
- **CakePHP 4 and 5:** tables, components, helpers, behaviours, associations,
  templates, layouts, elements, cells, configuration keys and route arrays by name
  (plugin dot notation too), with a warning for a template, a component, a helper, a
  behaviour or an action that names nothing; the types of `fetchTable()`, of a table's
  entities and queries, of an entity's fields (from the migrations), of association
  properties, of a controller's components and a view's helpers, and of the variables an
  action sets for its template. Translations (`__()`) by name; templates found in themes.
- **Laminas MVC and Mezzio:** routes, templates, services, controller plugins, view
  helpers, actions and configuration keys by name, from the modules' and providers'
  configuration, with a warning for a route, a template, a service or an action that
  names nothing; the types of `$container->get()` (through aliases, factories and
  invokables), of controller plugins and view helpers, and of the variables an action
  or a handler gives its `.phtml` template.
- **Magento 2:** events, configuration paths, ACL resources, templates, layout handles,
  block names and modules by name, going to the XML that declares them (`events.xml`,
  `system.xml`, `config.xml`, `acl.xml`, layout XML), with a warning for a template, an
  ACL resource or a module that isn't there and for a plugin method that intercepts
  nothing; the classes Magento generates (`FooFactory`, `Foo\Proxy`,
  `Foo\Interceptor`, extension-attribute interfaces) without a `generated/` folder; the
  types of the object manager, of a data object's setters, and of `$block` in a
  `.phtml` template, from the layout XML. The XML files are served too: goto, hover,
  usages, completion and rename on the classes and names in `di.xml`, layout files and
  the rest, with a warning for a class that doesn't exist; a class's usages and rename
  reach the XML, and a method's usages include its plugins.
- **Joomla:** language constants, layouts, web assets, extensions, events and their
  handlers, ACL actions, forms and their fields, parameters, tables and the parts of a
  URL by name, from the manifests, the language files, `access.xml`, the forms and
  `joomla.asset.json`, with a warning for a language constant or a layout that isn't
  there; the types of the application, of what a component's MVC factory makes, of a
  view's template (`$this`) and of an event handler's parameter. The XML, language and
  asset files are served too.
- **Drupal:** services, routes, permissions, libraries, configuration, entity types,
  plugins, theme hooks and hooks, from the module's YAML files, attributes and
  annotations; the types of `\Drupal::service()`, of an entity storage and of a hook
  implementation's parameters. `.module`, `.theme` and `.install` files are PHP.

## Performance

The server is one native binary: it needs no PHP or Node.js, and it keeps its index on
disk, so a project opened before starts in milliseconds.

<!-- bench:summary:start -->
On laravel/framework v13.40.0 (3,177 PHP files), release build, 2026-11-17, Windows x86_64, 16 threads:

|  | Phoxite | Intelephense 1.18.5 |
| --- | --- | --- |
| Index from an empty cache | 1.5 s | 4.1 s |
| Memory once indexed | 78 MB | 324 MB |
| Memory after hover, completion and diagnostics in a 2,500-line file | 179 MB | not measured |

| Phoxite, same project |  |
| --- | --- |
| Start on the index of an earlier run | 59 ms |
| Hover right after a keystroke | 2.3 ms |
| Completion right after a keystroke | 7.2 ms |
| Diagnostics after an edit (120 ms of it is the debounce) | 216 ms |
<!-- bench:summary:end -->

Intelephense got no requests after indexing, so its memory is that of the index alone.
`node scripts/bench_report.mjs` measures all of this again and rewrites the tables; it
needs the corpus (`scripts/php-corpus.sh laravel-framework`) and
`npm i -g intelephense`. The budgets behind the latencies, and what each one measures,
are in [docs/engine/performance.md](docs/engine/performance.md); CI fails when one is
exceeded.

## Layout

| Path | What |
| --- | --- |
| `crates/phoxite` | The engine: parsing (Mago), indexes (redb), types, inspections, refactorings, Laravel. |
| `crates/phoxite-lsp` | The language server over stdio. |
| `editors/vscode` | The VS Code extension (also Cursor, VSCodium, Windsurf through Open VSX). |
| `editors/openidea` | The Laravel tools as an OpenIDEA plugin. |
| `docs/editors.md` | Setting the server up in OpenIDEA, Neovim, Helix, Zed and Sublime Text. |
| `docs/frameworks.md` | What each framework's support knows: its kinds of name and its inspections. |
| `docs/settings.md` | Every setting of `phoxite.json`. |
| `docs/inspections.md` | Every inspection: its id, severity and defaults. |
| `docs/engine` | The engine's design notes and roadmap. |
| `docs/openidea.md` | How OpenIDEA uses Phoxite, and what a release must keep for it. |
| `CONTRIBUTING.md` | Building, testing and sending a change. |
| `CHANGELOG.md` | What each release changed. |

## Building

```sh
cargo build --release -p phoxite-lsp      # target/release/phoxite-lsp
cargo test --workspace
```

The VS Code extension, with a locally built server:

```sh
cd editors/vscode
npm install
npm run build
# Point "phoxite.server.path" at target/release/phoxite-lsp, then press F5
# in VS Code with this folder open, or package it:
node ../../scripts/package_vscode.mjs --server ../../target/release/phoxite-lsp
```

## Configuration

The server reads its settings from `initializationOptions.settings` and from
`workspace/didChangeConfiguration`, as one object or under a `phoxite` key:

```json
{
  "phpVersion": "8.3",
  "includePaths": ["../shared", "../../config.php"],
  "stubs": ["redis"],
  "exclude": ["node_modules", "/storage"],
  "inspections": { "PhpUndefinedMethod": { "enabled": false }, "PhpParams": { "strict": true } },
  "laravel": {
    "routeHints": true, "modules": "auto", "detection": "auto", "database": "",
    "generation": { "module": "", "controllerSuffix": "Controller", "typeHints": true }
  },
  "frameworks": { "laravel": "auto", "symfony": "auto" },
  "format": { "lineWidth": 120, "tool": "phoxite", "twig": "phoxite" },
  "inlayHints": { "arrayShapes": "compact" },
  "templates": { "path": ".phoxite/templates" },
  "analyzers": { "onSave": false, "phpPath": "", "nodePath": "" }
}
```

A type inlay hint shows an array shape as `array` (`array{…}|array{…}` reads `array`); its
tooltip and the hover keep the full type. `inlayHints.arrayShapes` set to `full` shows the
keys and their types in the hint too.

`initializationOptions.cacheDir` sets where the indexes live (default: the user's cache
folder, `phoxite/<hash of the folder>`).

A project can carry its settings itself, for everyone who works on it whatever their
editor: a `phoxite.json` at its root holds the same object.

```json
{
  "$schema": "https://raw.githubusercontent.com/samehelhawary/phoxite/main/editors/vscode/schemas/phoxite.schema.json",
  "phpVersion": "8.4",
  "stubs": ["redis"],
  "inspections": { "PhpUnusedParameter": { "enabled": false } }
}
```

What the file sets wins over the editor's settings, key by key, and the server follows
the file when it changes. A project doesn't choose the programs that run or the files
read outside it: `analyzers` and `laravel.database` are never taken from the file, and
`includePaths` only in a workspace the user trusts. A setting whose value doesn't fit is
named in a message and keeps its default.

## Checking a project from the command line

The same binary inspects a project without an editor, for a terminal or for CI:

```sh
phoxite-lsp check                      # the project of the current folder
phoxite-lsp check app routes           # only what is below these paths
phoxite-lsp check --format sarif > phoxite.sarif
```

It reports the problems of the project's own PHP files, Blade and Twig templates (not
`vendor/`) as an editor shows them, with the settings of the project's `phoxite.json`.
The exit code is 1 when there is an error or a warning; `--fail-on error` lets warnings
pass and `--fail-on never` only reports. `--format` takes `text`, `json`, `sarif`
(2.1.0, for code scanning) and `github`, which GitHub Actions shows as annotations on
the changed lines:

```yaml
- run: |
    curl -fsSLo phoxite-lsp https://github.com/samehelhawary/phoxite/releases/latest/download/phoxite-lsp-x86_64-unknown-linux-gnu
    chmod +x phoxite-lsp
    ./phoxite-lsp check --format github
```

Install the project's dependencies first (`composer install`): without `vendor/` every
class of a package is an undefined class. `phoxite-lsp check --help` lists the rest.

## Licence

Apache-2.0. See `NOTICE` and `THIRD_PARTY_LICENSES.md` for what is bundled (the
phpstorm-stubs, the Laravel generator templates).
