//! The names the project's files declare for a framework
//! ([`NamesProject`]): the [`NameFacts`] of every file that has some, with
//! the file each one is in.
//!
//! [`PhpEngine::names`] builds it from the index (and the open buffers) and
//! keeps it until something is edited. Nothing asks for it but the
//! reference kinds of a detected framework.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::base::Range;
use crate::engine::PhpEngine;
use crate::summary::{DeclaredKind, FileSummary, NameFacts, matches_pattern};

/// A name and where it is declared.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclaredAt {
    pub kind: DeclaredKind,
    /// With a `*` for each part that isn't written, when `dynamic`.
    pub name: String,
    pub dynamic: bool,
    pub path: PathBuf,
    /// The name's text.
    pub range: Range,
    /// In `vendor/` or another library folder: not the project's to rename.
    pub library: bool,
    /// [`DeclaredName::classes`](crate::summary::DeclaredName::classes).
    pub classes: Vec<String>,
}

impl DeclaredAt {
    /// Whether `name` is this one, or one its pattern gives.
    pub fn names(&self, name: &str) -> bool {
        if self.dynamic { matches_pattern(&self.name, name) } else { self.name == name }
    }
}

/// See the module documentation.
#[derive(Clone, Debug, Default)]
pub struct NamesProject {
    /// In path order, then source order.
    pub declared: Vec<DeclaredAt>,
    /// The actions of the AJAX requests a function or method is registered
    /// for by name (`shop_add` of `add_action('wp_ajax_shop_add',
    /// 'shop_add')`), sorted.
    pub ajax_actions: Vec<String>,
}

impl NamesProject {
    pub fn new<'a>(facts: impl IntoIterator<Item = (&'a Path, &'a NameFacts, bool)>) -> Self {
        let mut declared = Vec::new();
        let mut ajax_actions = Vec::new();
        for (path, facts, library) in facts {
            for callback in &facts.callbacks {
                if let Some(action) = callback.hook.strip_prefix("wp_ajax_") {
                    let action = action.strip_prefix("nopriv_").unwrap_or(action);
                    if !action.is_empty() && !library {
                        ajax_actions.push(action.to_owned());
                    }
                }
            }
            // `register_block_type('build/price')` names a folder with a `block.json`
            // when there is one, and a block otherwise.
            let folder = |name: &crate::summary::DeclaredName| {
                name.kind == DeclaredKind::Block && path.parent().is_some_and(|parent| parent.join(&name.name).is_dir())
            };
            declared.extend(facts.declared.iter().filter(|name| !folder(name)).map(|name| DeclaredAt {
                kind: name.kind,
                name: name.name.clone(),
                dynamic: name.dynamic,
                path: path.to_path_buf(),
                range: name.loc.range,
                library,
                classes: name.classes.clone(),
            }));
        }
        ajax_actions.sort();
        ajax_actions.dedup();
        Self { declared, ajax_actions }
    }

    /// The declarations of one of `kinds`.
    pub fn of<'p>(&'p self, kinds: &'p [DeclaredKind]) -> impl Iterator<Item = &'p DeclaredAt> {
        self.declared.iter().filter(move |declared| kinds.contains(&declared.kind))
    }

    /// Where `name` is declared as one of `kinds`: the places that write
    /// it, or else the dynamic names it can be one of.
    pub fn named<'p>(&'p self, kinds: &'p [DeclaredKind], name: &str) -> Vec<&'p DeclaredAt> {
        let exact: Vec<&DeclaredAt> =
            self.of(kinds).filter(|declared| !declared.dynamic && declared.name == name).collect();
        if !exact.is_empty() {
            return exact;
        }
        self.of(kinds).filter(|declared| declared.dynamic && declared.names(name)).collect()
    }
}

/// The class aliases of the project ([`DeclaredKind::ClassAlias`]): each
/// alias, in lower case without the leading `\`, to the class it stands for.
#[derive(Clone, Debug, Default)]
pub struct ClassAliases {
    /// The class, and whether a library declares the alias.
    aliases: HashMap<String, (String, bool)>,
}

impl ClassAliases {
    /// The project's own alias wins over a library's; between two of the
    /// same kind, the first in path order.
    pub fn new<'a>(facts: impl IntoIterator<Item = (&'a NameFacts, bool)>) -> Self {
        let mut aliases: HashMap<String, (String, bool)> = HashMap::new();
        for (facts, library) in facts {
            for alias in facts.declared.iter().filter(|name| name.kind == DeclaredKind::ClassAlias) {
                let Some(class) = alias.classes.first() else { continue };
                let key = alias_key(&alias.name);
                match aliases.get(&key) {
                    Some((_, known_library)) if !*known_library || library => {}
                    _ => {
                        aliases.insert(key, (class.clone(), library));
                    }
                }
            }
        }
        Self { aliases }
    }

    /// The class `name` is an alias of, following an alias of an alias to
    /// the end; `None` when `name` is no alias or the aliases make a loop.
    pub fn resolve(&self, name: &str) -> Option<String> {
        let mut class = &self.aliases.get(&alias_key(name))?.0;
        for _ in 0..ALIAS_HOPS {
            match self.aliases.get(&alias_key(class)) {
                Some((next, _)) => class = next,
                None => return Some(class.clone()),
            }
        }
        None
    }
}

/// How a class alias is looked up in [`ClassAliases`].
fn alias_key(name: &str) -> String {
    name.trim_start_matches('\\').to_ascii_lowercase()
}

/// How many aliases of aliases [`ClassAliases::resolve`] follows.
const ALIAS_HOPS: usize = 8;

impl PhpEngine {
    /// The class `name` is an alias of (`class_alias()`); see
    /// [`ClassAliases::resolve`].
    pub(crate) fn class_alias(&self, name: &str) -> Option<String> {
        self.class_aliases().resolve(name)
    }

    /// The project's class aliases, kept until something is edited. Read
    /// from the few files the index lists as declaring some (and the open
    /// buffers), not from every file with [`NameFacts`]: every class lookup
    /// that finds no class asks.
    pub(crate) fn class_aliases(&self) -> Arc<ClassAliases> {
        let count = self.edit_count();
        if let Some((at, aliases)) = self.shared_class_aliases().lock().clone()
            && at == count
        {
            return aliases;
        }
        let indexed = self.index().and_then(|index| index.class_alias_files().ok()).unwrap_or_default();
        let mut summaries: Vec<(PathBuf, Arc<FileSummary>)> =
            indexed.iter().filter_map(|path| Some((path.clone(), self.shared_summary(path).ok()??))).collect();
        for (path, summary) in self.open_summaries() {
            let has = summary.names.declared.iter().any(|name| name.kind == DeclaredKind::ClassAlias);
            if has && !indexed.contains(&path) {
                summaries.push((path, summary));
            }
        }
        summaries.sort_by(|a, b| a.0.cmp(&b.0));
        let aliases = Arc::new(ClassAliases::new(
            summaries.iter().map(|(path, summary)| (&summary.names, self.is_library(path))),
        ));
        *self.shared_class_aliases().lock() = Some((count, aliases.clone()));
        aliases
    }

    /// The names the project's files declare; see the module documentation.
    pub fn names(&self) -> Arc<NamesProject> {
        let count = self.edit_count();
        if let Some((at, project)) = self.shared_names().lock().clone()
            && at == count
        {
            return project;
        }
        let indexed = self.index().and_then(|index| index.names_files().ok()).unwrap_or_default();
        let mut summaries: Vec<(PathBuf, Arc<FileSummary>)> = {
            use rayon::prelude::*;
            indexed.par_iter().filter_map(|path| Some((path.clone(), self.shared_summary(path).ok()??))).collect()
        };
        for (path, summary) in self.open_summaries() {
            if !indexed.contains(&path) && !summary.names.is_empty() {
                summaries.push((path, summary));
            }
        }
        summaries.sort_by(|a, b| a.0.cmp(&b.0));
        let project = Arc::new(NamesProject::new(
            summaries.iter().map(|(path, summary)| (path.as_path(), &summary.names, self.is_library(path))),
        ));
        *self.shared_names().lock() = Some((count, project.clone()));
        project
    }
}
