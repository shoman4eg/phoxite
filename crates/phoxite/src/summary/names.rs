//! The names a file declares for a framework ([`NameFacts`]): things a
//! string names elsewhere and that no PHP declaration stands for, like a
//! WordPress hook, which exists where `do_action('name')` fires it.
//!
//! One struct for every framework, so that a new kind of name is a
//! [`DeclaredKind`] and a row of the extraction table
//! (`summary/extract/names.rs`), not another field of [`FileSummary`], index
//! key and cache. Recorded for any file (a summary can't know which
//! frameworks the project uses); the index lists the files that have some,
//! and only a detected framework's reference kinds read them
//! ([`crate::frameworks::names`]).
//!
//! The types of global variables are facts of the same kind: a variable is
//! `global` in one function and documented (`@global wpdb $wpdb`) in
//! another, often of another file.
//!
//! Only declarations are facts: the usages of a name are found like those
//! of any string reference, through the word index.
//!
//! [`FileSummary`]: super::FileSummary

use serde::{Deserialize, Serialize};

use super::Loc;

/// See the module documentation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NameFacts {
    /// In source order.
    pub declared: Vec<DeclaredName>,
    /// The global variables the `@global Type $name` tags of the file's
    /// functions and methods document, each name and type once.
    #[serde(default)]
    pub globals: Vec<GlobalVariable>,
    /// The functions and methods the file registers as callbacks of a
    /// hook by name (`add_action('save_post', 'shop_saved')`).
    #[serde(default)]
    pub callbacks: Vec<HookCallback>,
}

impl NameFacts {
    pub fn is_empty(&self) -> bool {
        self.declared.is_empty() && self.globals.is_empty() && self.callbacks.is_empty()
    }
}

/// A function or method registered as a hook's callback.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookCallback {
    pub hook: String,
    /// In lower case: a function's full name, or `class::method` with the
    /// class's full name. See [`callback_key`].
    pub callback: String,
}

/// What comes before the name of a Drupal hook in [`HookCallback::hook`]:
/// `drupal:node_presave` for a method with `#[Hook('node_presave')]`.
pub const DRUPAL_HOOK: &str = "drupal:";

/// How a function (`class` is `None`) or a method is named in
/// [`HookCallback::callback`].
pub fn callback_key(class: Option<&str>, name: &str) -> String {
    let name = name.trim_start_matches('\\').to_ascii_lowercase();
    match class {
        Some(class) => format!("{}::{name}", class.trim_start_matches('\\').to_ascii_lowercase()),
        None => name,
    }
}

/// A global variable as a `@global` tag documents it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalVariable {
    /// Without the `$`.
    pub name: String,
    pub ty: super::DocType,
}

/// What a [`DeclaredName`] is the name of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeclaredKind {
    /// A WordPress action: `do_action('name')`.
    Action,
    /// A WordPress filter: `apply_filters('name', $value)`.
    Filter,
    /// A WordPress shortcode: `add_shortcode('name', …)`.
    Shortcode,
    /// A WordPress post type: `register_post_type('name', …)`.
    PostType,
    /// A WordPress taxonomy: `register_taxonomy('name', …)`.
    Taxonomy,
    /// A WordPress script handle: `wp_register_script('name', …)`.
    Script,
    /// A WordPress style handle: `wp_register_style('name', …)`.
    Style,
    /// A WordPress option: `add_option('name', …)`, `update_option()`,
    /// `register_setting()`.
    Option,
    /// A WordPress image size: `add_image_size('name', …)`.
    ImageSize,
    /// A WordPress menu location: `register_nav_menu('name', …)`.
    NavMenu,
    /// A WordPress block: `register_block_type('namespace/name', …)`.
    Block,
    /// A WordPress REST route: `register_rest_route('shop/v1', '/books',
    /// …)` is `shop/v1/books`.
    RestRoute,
    /// A CodeIgniter route's name: the `as` of its options,
    /// `$routes->get('books', 'Books::index', ['as' => 'books'])`.
    CodeIgniterRoute,
    /// A CodeIgniter event: what `Events::trigger('name')` fires.
    CodeIgniterEvent,
    /// CodeIgniter 3: the name a model is loaded under,
    /// `$this->load->model('book_model', 'books')`. Its class is the model.
    CodeIgniterLoaded,
    /// A table a CodeIgniter migration makes: `$this->forge->createTable('books')`.
    CodeIgniterTable,
    /// A column of such a table: a key of the fields given to `addField()`
    /// before it, as `table.column`.
    CodeIgniterColumn,
    /// A Drupal entity type: the `id` of a `#[ContentEntityType]` attribute
    /// or annotation. Its [`classes`](DeclaredName::classes) are the entity
    /// class and the storage handler (empty when none is named).
    DrupalEntityType,
    /// A Drupal plugin: the `id` of the attribute or annotation of a class
    /// in a `Plugin` namespace. Its classes are the plugin's class and the
    /// attribute's short name (`Block`).
    DrupalPlugin,
    /// A Drupal 7 permission: a key of the array `hook_permission()`
    /// returns.
    DrupalPermission,
    /// A Drupal 7 menu path: a key of what `hook_menu()` returns or fills
    /// (`$items['admin/shop'] = array(…)`).
    DrupalMenuPath,
    /// A Drupal 7 variable: `variable_set('name', …)`.
    DrupalVariable,
    /// A table of Drupal's database: a key of what `hook_schema()` returns
    /// or fills (`$schema['shop_book'] = array(…)`).
    DrupalTable,
    /// A column of such a table: a key of its `fields`, as `table.column`.
    DrupalColumn,
    /// A Drupal theme hook: a key of the array `hook_theme()` returns. Its
    /// classes are the variables its template is given.
    DrupalThemeHook,
    /// A class alias: `class_alias('Real', 'Alias')`. The name is the
    /// alias's full name without the leading `\`; its one class is the
    /// class it stands for. Not framework specific: every project reads
    /// them ([`crate::engine::PhpEngine`]'s class lookup).
    ClassAlias,
}

/// A name a call declares.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclaredName {
    pub kind: DeclaredKind,
    /// The name. When `dynamic`, with a `*` for each part that isn't
    /// written (`save_post_*` for `"save_post_{$post->post_type}"`).
    pub name: String,
    /// The name is put together at run time.
    pub dynamic: bool,
    /// The name's string, without its quotes (a concatenation: all of it).
    pub loc: Loc,
    /// For a hook: the types of what it passes its callbacks, as the
    /// `@param` tags of the docblock in front of the firing statement give
    /// them, in their order.
    #[serde(default)]
    pub params: Vec<super::DocType>,
    /// What the name stands for, by kind (see [`DeclaredKind`]); empty for
    /// most.
    #[serde(default)]
    pub classes: Vec<String>,
}

impl DeclaredName {
    /// Whether `name` is this one: the same, or one the pattern of a
    /// dynamic name gives.
    pub fn names(&self, name: &str) -> bool {
        if !self.dynamic {
            return self.name == name;
        }
        matches_pattern(&self.name, name)
    }
}

/// Whether `name` is `pattern`, each `*` of it standing for one or more
/// characters.
pub fn matches_pattern(pattern: &str, name: &str) -> bool {
    let mut parts = pattern.split('*');
    let Some(first) = parts.next() else { return false };
    let Some(mut rest) = name.strip_prefix(first) else { return false };
    let mut parts = parts.peekable();
    if parts.peek().is_none() {
        return rest.is_empty();
    }
    while let Some(part) = parts.next() {
        // A `*` stands for something.
        let mut chars = rest.chars();
        if chars.next().is_none() {
            return false;
        }
        let after = chars.as_str();
        if parts.peek().is_none() {
            return part.is_empty() || after.ends_with(part) || rest.len() > part.len() && rest.ends_with(part);
        }
        if part.is_empty() {
            continue;
        }
        let Some(at) = after.find(part) else { return false };
        rest = &after[at + part.len()..];
    }
    true
}

#[cfg(test)]
mod tests {
    use super::matches_pattern;

    #[test]
    fn dynamic_names_match_as_patterns() {
        assert!(matches_pattern("save_post_*", "save_post_book"));
        assert!(!matches_pattern("save_post_*", "save_post_"));
        assert!(!matches_pattern("save_post_*", "save_page"));
        assert!(matches_pattern("*_edit_form", "category_edit_form"));
        assert!(!matches_pattern("*_edit_form", "_edit_form"));
        assert!(matches_pattern("manage_*_posts_columns", "manage_book_posts_columns"));
        assert!(!matches_pattern("manage_*_posts_columns", "manage_book_posts_rows"));
        assert!(matches_pattern("a_*_b_*", "a_x_b_y"));
        assert!(!matches_pattern("a_*_b_*", "a_x_b_"));
        assert!(matches_pattern("plain", "plain"));
        assert!(!matches_pattern("plain", "plainer"));
    }
}
