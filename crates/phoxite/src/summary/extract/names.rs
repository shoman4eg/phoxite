//! The names a file declares for a framework
//! ([`NameFacts`](crate::summary::NameFacts)), read from the calls of
//! [`DECLARING`].
//!
//! The functions are the ones WordPress's developer handbooks document for
//! firing hooks and for registering shortcodes, post types, taxonomies,
//! scripts, styles, options, image sizes, menu locations and blocks. Drupal's
//! entity types, plugins and theme hooks are read from the attributes and
//! annotations of classes and from `hook_theme()` (the second `impl`). They are
//! matched by name only: a summary doesn't resolve calls, and whether the
//! project is a WordPress one is asked when the facts are read.

use mago_span::{HasSpan, Span};
use mago_syntax::comments::docblock::get_docblock_before_position;
use mago_syntax::cst::{
    Argument, ArrayElement, AttributeList, Block, CompositeString, Expression, FunctionCall, Literal, PartialArgument,
    Sequence, Statement, StringPart,
};

use super::Extractor;
use crate::summary::{DeclaredKind, DeclaredName, DocBlock, GlobalVariable};

/// Which part of the argument is the name.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Form {
    /// The argument.
    Argument,
    /// The argument, when another follows it: `wp_enqueue_script('app',
    /// $src)` registers the script, `wp_enqueue_script('app')` only uses it.
    WithNext,
    /// Each key of the array the argument is.
    Keys,
}

/// A function, the position of the argument that names, how, and what the
/// name is of.
const DECLARING: &[(&str, usize, Form, DeclaredKind)] = &[
    ("do_action", 0, Form::Argument, DeclaredKind::Action),
    ("do_action_ref_array", 0, Form::Argument, DeclaredKind::Action),
    ("do_action_deprecated", 0, Form::Argument, DeclaredKind::Action),
    ("apply_filters", 0, Form::Argument, DeclaredKind::Filter),
    ("apply_filters_ref_array", 0, Form::Argument, DeclaredKind::Filter),
    ("apply_filters_deprecated", 0, Form::Argument, DeclaredKind::Filter),
    ("add_shortcode", 0, Form::Argument, DeclaredKind::Shortcode),
    ("register_post_type", 0, Form::Argument, DeclaredKind::PostType),
    ("register_taxonomy", 0, Form::Argument, DeclaredKind::Taxonomy),
    ("wp_register_script", 0, Form::Argument, DeclaredKind::Script),
    ("wp_enqueue_script", 0, Form::WithNext, DeclaredKind::Script),
    ("wp_register_style", 0, Form::Argument, DeclaredKind::Style),
    ("wp_enqueue_style", 0, Form::WithNext, DeclaredKind::Style),
    ("add_option", 0, Form::Argument, DeclaredKind::Option),
    ("update_option", 0, Form::Argument, DeclaredKind::Option),
    ("register_setting", 1, Form::Argument, DeclaredKind::Option),
    ("add_image_size", 0, Form::Argument, DeclaredKind::ImageSize),
    ("register_nav_menu", 0, Form::Argument, DeclaredKind::NavMenu),
    ("register_nav_menus", 0, Form::Keys, DeclaredKind::NavMenu),
    ("register_block_type", 0, Form::Argument, DeclaredKind::Block),
    // Drupal 7.
    ("variable_set", 0, Form::Argument, DeclaredKind::DrupalVariable),
];

/// The part of a dynamic name that isn't written.
const UNKNOWN: char = '*';

impl Extractor<'_> {
    /// The names `node` declares, if it is a call of [`DECLARING`].
    pub(super) fn declared_name(&mut self, node: &FunctionCall<'_>) {
        let Expression::Identifier(function) = node.function else { return };
        let Some(called) = function.value().rsplit(|byte| *byte == b'\\').next() else { return };
        // `Foo\class_alias()` is some other function.
        let builtin = function.value().strip_prefix(b"\\").unwrap_or(function.value()) == called;
        if builtin && called.eq_ignore_ascii_case(b"class_alias") {
            self.class_alias(node);
            return;
        }
        // Every one starts with one of these letters: most calls end here.
        if !matches!(called.first().map(u8::to_ascii_lowercase), Some(b'a' | b'd' | b'r' | b'u' | b'v' | b'w')) {
            return;
        }
        if called.eq_ignore_ascii_case(b"add_action") || called.eq_ignore_ascii_case(b"add_filter") {
            self.hook_callback(node);
            return;
        }
        if called.eq_ignore_ascii_case(b"register_rest_route") {
            self.rest_route(node);
            return;
        }
        let Some((_, index, form, kind)) =
            DECLARING.iter().find(|(known, ..)| called.eq_ignore_ascii_case(known.as_bytes()))
        else {
            return;
        };
        let arguments = node.argument_list.arguments.as_slice();
        let Some(Argument::Positional(argument)) = arguments.get(*index) else { return };
        match form {
            Form::WithNext if arguments.len() <= index + 1 => {}
            Form::Argument | Form::WithNext => {
                self.declare(*kind, argument.value);
                if matches!(kind, DeclaredKind::Action | DeclaredKind::Filter) {
                    self.hook_parameters(node);
                }
            }
            Form::Keys => {
                let elements = match argument.value {
                    Expression::Array(array) => &array.elements,
                    Expression::LegacyArray(array) => &array.elements,
                    _ => return,
                };
                for element in elements.iter() {
                    if let ArrayElement::KeyValue(element) = element {
                        self.declare(*kind, element.key);
                    }
                }
            }
        }
    }

    /// The function or method `add_action('hook', …)` or `add_filter()`
    /// registers by name: `'function'`, `'Class::method'`, `[$this,
    /// 'method']`, `[Foo::class, 'method']`, `['Foo', 'method']`.
    fn hook_callback(&mut self, node: &FunctionCall<'_>) {
        let arguments = node.argument_list.arguments.as_slice();
        let (Some(Argument::Positional(hook)), Some(Argument::Positional(callback))) =
            (arguments.first(), arguments.get(1))
        else {
            return;
        };
        let text = |expr: &Expression<'_>| match expr {
            Expression::Literal(Literal::String(string)) => {
                string.value.map(|v| String::from_utf8_lossy(v).into_owned())
            }
            _ => None,
        };
        let Some(hook) = text(hook.value).filter(|hook| !hook.is_empty()) else { return };
        let callback = match callback.value {
            Expression::Array(_) | Expression::LegacyArray(_) => {
                let elements = match callback.value {
                    Expression::Array(array) => &array.elements,
                    Expression::LegacyArray(array) => &array.elements,
                    _ => return,
                };
                let mut values = elements.iter().map(|element| match element {
                    ArrayElement::Value(element) => Some(element.value),
                    _ => None,
                });
                let (Some(Some(object)), Some(Some(method)), None) = (values.next(), values.next(), values.next())
                else {
                    return;
                };
                let Some(method) = text(method) else { return };
                let own = || self.classes.last().cloned();
                let class = match object {
                    Expression::Variable(mago_syntax::cst::Variable::Direct(variable)) if variable.name == b"$this" => {
                        own()
                    }
                    Expression::Access(mago_syntax::cst::Access::ClassConstant(access))
                        if matches!(&access.constant, mago_syntax::cst::ClassLikeConstantSelector::Identifier(name)
                            if name.value.eq_ignore_ascii_case(b"class")) =>
                    {
                        match access.class {
                            Expression::Identifier(class) => Some(self.class_name(class)),
                            Expression::Self_(_) | Expression::Static(_) => own(),
                            _ => None,
                        }
                    }
                    other => text(other),
                };
                let Some(class) = class else { return };
                crate::summary::callback_key(Some(&class), &method)
            }
            other => {
                let Some(written) = text(other) else { return };
                match written.split_once("::") {
                    Some((class, method)) => crate::summary::callback_key(Some(class), method),
                    None => crate::summary::callback_key(None, &written),
                }
            }
        };
        self.summary.names.callbacks.push(crate::summary::HookCallback { hook, callback });
    }

    /// The route `register_rest_route('shop/v1', '/books/(?P<id>\d+)', …)`
    /// declares: `shop/v1/books/*`, at the route's string.
    fn rest_route(&mut self, node: &FunctionCall<'_>) {
        let arguments = node.argument_list.arguments.as_slice();
        let (Some(Argument::Positional(namespace)), Some(Argument::Positional(route))) =
            (arguments.first(), arguments.get(1))
        else {
            return;
        };
        let source = self.file.text();
        let namespace = rest_text(namespace.value, source);
        let namespace = namespace.trim_matches('/');
        let pattern = route_pattern(&rest_text(route.value, source));
        // A namespace that isn't known would make the route anything's.
        if namespace.is_empty() || namespace.contains(UNKNOWN) || pattern.is_empty() {
            return;
        }
        let name = format!("{namespace}/{pattern}");
        let dynamic = name.contains(UNKNOWN);
        let loc = self.loc(inside(route.value));
        self.summary.names.declared.push(DeclaredName {
            kind: DeclaredKind::RestRoute,
            name,
            dynamic,
            loc,
            params: Vec::new(),
            classes: Vec::new(),
        });
    }

    /// The global variables the `@global` tags of the file's functions and
    /// methods document.
    pub(super) fn documented_globals(&mut self) {
        let mut found: Vec<GlobalVariable> = Vec::new();
        let mut add = |doc: Option<&DocBlock>| {
            for global in doc.iter().flat_map(|doc| &doc.globals) {
                let Some(name) = &global.name else { continue };
                if !found.iter().any(|known| known.name == *name && known.ty == global.ty) {
                    found.push(GlobalVariable { name: name.clone(), ty: global.ty.clone() });
                }
            }
        };
        for function in &self.summary.functions {
            add(function.doc.as_deref());
        }
        for class in self.summary.class_likes.iter().chain(&self.summary.anonymous) {
            for method in &class.methods {
                add(method.doc.as_deref());
            }
        }
        self.summary.names.globals = found;
    }

    fn declare(&mut self, kind: DeclaredKind, value: &Expression<'_>) {
        let mut name = String::new();
        let mut dynamic = false;
        pattern(value, &mut name, &mut dynamic);
        // Only a hook is listened to by a name its firing call puts together.
        let hook = matches!(kind, DeclaredKind::Action | DeclaredKind::Filter);
        if name.is_empty() || name.chars().all(|c| c == UNKNOWN) || (dynamic && !hook) {
            return;
        }
        // `register_block_type()` also takes the folder of a `block.json`.
        if kind == DeclaredKind::Block && !block_name(&name) {
            return;
        }
        let loc = self.loc(inside(value));
        self.summary.names.declared.push(DeclaredName {
            kind,
            name,
            dynamic,
            loc,
            params: Vec::new(),
            classes: Vec::new(),
        });
    }

    /// Gives the hook just declared by `node` the `@param` types of the
    /// docblock in front of its statement: where WordPress's documentation
    /// standard puts what a hook passes.
    fn hook_parameters(&mut self, node: &FunctionCall<'_>) {
        let start = node.span().start.offset;
        let Some(declared) = self.summary.names.declared.last().filter(|declared| declared.params.is_empty()) else {
            return;
        };
        if declared.loc.start < start {
            return;
        }
        // The statement starts the line: `$title = apply_filters(…)`, `return apply_filters(…)`.
        let text = self.file.text().as_bytes();
        let from = (start as usize).min(text.len());
        let line = text[..from].iter().rposition(|byte| *byte == b'\n').map_or(0, |at| at + 1);
        let statement = line + text[line..from].iter().take_while(|byte| byte.is_ascii_whitespace()).count();
        let Some(doc) = self.doc_at(statement as u32, &[]) else { return };
        let params: Vec<_> = doc.params.iter().map(|param| param.ty.clone()).collect();
        if let Some(declared) = self.summary.names.declared.last_mut() {
            declared.params = params;
        }
    }
}

/// What `expr` says, with [`UNKNOWN`] for what isn't written: literals,
/// their concatenation, and a property of `$this` that `source` sets to
/// one string ([`property_string`]).
fn rest_text(expr: &Expression<'_>, source: &str) -> String {
    use mago_syntax::cst::{Access, ClassLikeMemberSelector, Variable};
    match expr {
        Expression::Parenthesized(inner) => rest_text(inner.expression, source),
        Expression::Literal(Literal::String(string)) => match string.value {
            Some(value) => String::from_utf8_lossy(value).into_owned(),
            None => UNKNOWN.to_string(),
        },
        Expression::Binary(binary) if binary.operator.is_concatenation() => {
            format!("{}{}", rest_text(binary.lhs, source), rest_text(binary.rhs, source))
        }
        Expression::Access(Access::Property(access)) => {
            let name = match (&access.property, access.object) {
                (ClassLikeMemberSelector::Identifier(name), Expression::Variable(Variable::Direct(this)))
                    if this.name == b"$this" =>
                {
                    Some(String::from_utf8_lossy(name.value).into_owned())
                }
                _ => None,
            };
            name.and_then(|name| property_string(source, &name)).unwrap_or_else(|| UNKNOWN.to_string())
        }
        _ => UNKNOWN.to_string(),
    }
}

/// The string the file's text sets property `name` to, when every
/// `$this->name = '…';` and `$name = '…';` of it says the same one.
fn property_string(source: &str, name: &str) -> Option<String> {
    let mut found: Option<&str> = None;
    for written in [format!("$this->{name}"), format!("${name}")] {
        let mut from = 0;
        while let Some(at) = source[from..].find(&written) {
            from += at + written.len();
            let rest = &source[from..];
            if rest.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
                continue;
            }
            let Some(value) = rest.trim_start().strip_prefix('=') else { continue };
            if value.starts_with(['=', '>']) {
                continue;
            }
            let value = value.trim_start();
            let quote = value.chars().next().filter(|c| matches!(c, '\'' | '"'))?;
            let end = value[1..].find(quote)?;
            let string = &value[1..1 + end];
            let plain = !string.contains(['$', '\\']) && value[2 + end..].trim_start().starts_with(';');
            if !plain || found.is_some_and(|known| known != string) {
                return None;
            }
            found = Some(string);
        }
    }
    found.map(str::to_owned)
}

/// A REST route as a name: without the slashes around it, and with
/// [`UNKNOWN`] for each part a pattern stands for (a group, a character
/// class, and what repeats them).
fn route_pattern(route: &str) -> String {
    let mut out = String::new();
    let unknown = |out: &mut String| {
        if !out.ends_with(UNKNOWN) {
            out.push(UNKNOWN);
        }
    };
    let mut chars = route.trim_matches('/').chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '(' | '[' => {
                let (open, close) = if c == '(' { ('(', ')') } else { ('[', ']') };
                let mut depth = 1;
                while depth > 0
                    && let Some(inner) = chars.next()
                {
                    match inner {
                        '\\' => {
                            chars.next();
                        }
                        inner if inner == open => depth += 1,
                        inner if inner == close => depth -= 1,
                        _ => {}
                    }
                }
                unknown(&mut out);
            }
            '\\' => {
                // An escaped character is itself; a class (`\d`) is something.
                match chars.next() {
                    Some(escaped) if escaped.is_ascii_alphanumeric() => unknown(&mut out),
                    Some(escaped) => out.push(escaped),
                    None => {}
                }
            }
            '?' | '+' | '*' | '^' | '$' => {
                if matches!(c, '+' | '*') {
                    unknown(&mut out);
                }
            }
            '{' => {
                for inner in chars.by_ref() {
                    if inner == '}' {
                        break;
                    }
                }
            }
            '.' => unknown(&mut out),
            other => out.push(other),
        }
    }
    out
}

/// Whether `name` is a block's: `namespace/name`, in lower case letters,
/// digits and dashes.
fn block_name(name: &str) -> bool {
    let part = |part: &str| {
        part.starts_with(|c: char| c.is_ascii_lowercase())
            && part.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    };
    name.split_once('/').is_some_and(|(namespace, block)| part(namespace) && part(block))
}

/// Appends what `expr` says to `name`: the text of literals, and
/// [`UNKNOWN`] once for whatever else is there.
fn pattern(expr: &Expression<'_>, name: &mut String, dynamic: &mut bool) {
    let mut unknown = |name: &mut String| {
        *dynamic = true;
        if !name.ends_with(UNKNOWN) {
            name.push(UNKNOWN);
        }
    };
    match expr {
        Expression::Parenthesized(inner) => pattern(inner.expression, name, dynamic),
        Expression::Literal(Literal::String(string)) => match string.value {
            Some(value) => name.push_str(&String::from_utf8_lossy(value)),
            None => unknown(name),
        },
        Expression::CompositeString(CompositeString::Interpolated(string)) => {
            for part in string.parts.iter() {
                match part {
                    StringPart::Literal(literal) => match literal.value {
                        Some(value) => name.push_str(&String::from_utf8_lossy(value)),
                        None => unknown(name),
                    },
                    _ => unknown(name),
                }
            }
        }
        Expression::Binary(binary) if binary.operator.is_concatenation() => {
            pattern(binary.lhs, name, dynamic);
            pattern(binary.rhs, name, dynamic);
        }
        _ => unknown(name),
    }
}

/// The span of a name's text: inside the quotes of a string, else the
/// whole expression.
fn inside(expr: &Expression<'_>) -> Span {
    let mut span = expr.span();
    let quoted = match expr {
        Expression::Literal(Literal::String(string)) => string.raw.len() >= 2,
        Expression::CompositeString(CompositeString::Interpolated(_)) => true,
        _ => false,
    };
    if quoted && span.end.offset >= span.start.offset + 2 {
        span.start.offset += 1;
        span.end.offset -= 1;
    }
    span
}

/// The attributes and annotations that declare a Drupal entity type, by
/// the last part of their name.
const ENTITY_TYPES: [&str; 3] = ["ContentEntityType", "ConfigEntityType", "EntityType"];

impl Extractor<'_> {
    /// What the attributes and the annotation of the class `class` (FQN),
    /// which starts at `start`, declare for Drupal: an entity type
    /// (`#[ContentEntityType(id: 'node', handlers: ['storage' => …])]`,
    /// `@ContentEntityType(id = "node", …)`), or a plugin of a class in a
    /// `Plugin` namespace (`#[Block(id: 'shop_cart')]`, `@Block(id =
    /// "shop_cart")`).
    pub(super) fn drupal_class(&mut self, class: &str, start: u32, lists: &Sequence<'_, AttributeList<'_>>) {
        let plugin = class.contains("\\Plugin\\");
        let declare =
            |this: &mut Self, annotation: &str, loc: crate::summary::Loc, id: String, storage: Option<String>| {
                let short = annotation.rsplit('\\').next().unwrap_or(annotation);
                let entity = ENTITY_TYPES.contains(&short);
                if id.is_empty() || !(entity || plugin) {
                    return;
                }
                let (kind, other) = if entity {
                    (DeclaredKind::DrupalEntityType, storage.unwrap_or_default())
                } else {
                    (DeclaredKind::DrupalPlugin, short.to_owned())
                };
                this.summary.names.declared.push(DeclaredName {
                    kind,
                    name: id,
                    dynamic: false,
                    loc,
                    params: Vec::new(),
                    classes: vec![class.to_owned(), other],
                });
            };
        for attribute in lists.iter().flat_map(|list| list.attributes.iter()) {
            let Some(arguments) = &attribute.argument_list else { continue };
            let named = |wanted: &[u8]| {
                arguments.arguments.iter().find_map(|argument| match argument {
                    PartialArgument::Named(argument) if argument.name.value == wanted => Some(argument.value),
                    _ => None,
                })
            };
            let first = || match arguments.arguments.first() {
                Some(PartialArgument::Positional(argument)) => Some(argument.value),
                _ => None,
            };
            let Some(id) = named(b"id").or_else(first) else { continue };
            let Expression::Literal(Literal::String(string)) = id else { continue };
            let Some(name) = string.value.map(|value| String::from_utf8_lossy(value).into_owned()) else { continue };
            // `handlers: ['storage' => NodeStorage::class]`.
            let storage = named(b"handlers").and_then(|handlers| {
                let elements = match handlers {
                    Expression::Array(array) => &array.elements,
                    Expression::LegacyArray(array) => &array.elements,
                    _ => return None,
                };
                elements.iter().find_map(|element| {
                    let ArrayElement::KeyValue(element) = element else { return None };
                    let Expression::Literal(Literal::String(key)) = element.key else { return None };
                    (key.value == Some(b"storage".as_slice())).then(|| self.class_of(element.value)).flatten()
                })
            });
            let annotation = self.class_name(&attribute.name);
            let loc = self.loc(inside(id));
            declare(self, &annotation, loc, name, storage);
        }
        // The annotation of the class's docblock, as Drupal wrote plugins before attributes.
        let Some(trivia) = get_docblock_before_position(self.file.program().trivia.as_slice(), start) else { return };
        let text = String::from_utf8_lossy(trivia.value);
        let Some((annotation, body)) = annotation(&text) else { return };
        let Some((at, id)) = assigned(body, "id", false) else { return };
        let storage = assigned(body, "storage", true).map(|(_, class)| class.trim_start_matches('\\').to_owned());
        let from = trivia.span.start.offset + (text.len() - body.len() + at) as u32;
        let mut span = trivia.span;
        span.start.offset = from;
        span.end.offset = from + id.len() as u32;
        let loc = self.loc(span);
        declare(self, annotation, loc, id.to_owned(), storage);
    }

    /// The class `expr` names: `Foo::class`, or a string.
    fn class_of(&self, expr: &Expression<'_>) -> Option<String> {
        match expr {
            Expression::Access(mago_syntax::cst::Access::ClassConstant(access)) => match access.class {
                Expression::Identifier(class) => Some(self.class_name(class)),
                _ => None,
            },
            Expression::Literal(Literal::String(string)) => {
                string.value.map(|value| String::from_utf8_lossy(value).trim_start_matches('\\').to_owned())
            }
            _ => None,
        }
    }

    /// The theme hooks a `hook_theme()` implementation declares: the keys
    /// of the array that the function named `…_theme`, or the method with
    /// a `#[Hook('theme')]` attribute, returns.
    pub(super) fn theme_hooks(&mut self, name: &[u8], lists: &Sequence<'_, AttributeList<'_>>, body: &Block<'_>) {
        let hooked = lists.iter().flat_map(|list| list.attributes.iter()).any(|attribute| {
            let named = attribute.name.value().rsplit(|byte| *byte == b'\\').next() == Some(b"Hook".as_slice());
            let first = attribute.argument_list.as_ref().and_then(|list| list.arguments.first());
            named
                && matches!(first, Some(PartialArgument::Positional(argument))
                    if matches!(argument.value, Expression::Literal(Literal::String(string)) if string.value == Some(b"theme".as_slice())))
        });
        // Drupal 7's `hook_permission()` and `hook_menu()`: the keys of what they return, each
        // described by an array, and those of the array they fill (`$items['path'] = array(…)`).
        let described = if name.len() > 11 && name.ends_with(b"_permission") {
            Some(DeclaredKind::DrupalPermission)
        } else if name.len() > 5 && name.ends_with(b"_menu") {
            Some(DeclaredKind::DrupalMenuPath)
        } else if name.len() > 7 && name.ends_with(b"_schema") {
            Some(DeclaredKind::DrupalTable)
        } else {
            None
        };
        if let Some(kind) = described {
            let array = |expr: &Expression<'_>| matches!(expr, Expression::Array(_) | Expression::LegacyArray(_));
            for statement in body.statements.iter() {
                match statement {
                    Statement::Return(returned) => {
                        let elements = match returned.value {
                            Some(Expression::Array(array)) => &array.elements,
                            Some(Expression::LegacyArray(array)) => &array.elements,
                            _ => continue,
                        };
                        for element in elements.iter() {
                            let ArrayElement::KeyValue(element) = element else { continue };
                            if array(element.value) {
                                self.describe(kind, element.key, element.value);
                            }
                        }
                    }
                    Statement::Expression(statement) => {
                        let Expression::Assignment(assignment) = statement.expression else { continue };
                        match assignment.lhs {
                            // `$items['admin/shop'] = array(…);`
                            Expression::ArrayAccess(access) => {
                                if array(assignment.rhs) && matches!(access.array, Expression::Variable(_)) {
                                    self.describe(kind, access.index, assignment.rhs);
                                }
                            }
                            // `$perms = array('administer shop' => array(…));`, returned later.
                            Expression::Variable(_) => {
                                let elements = match assignment.rhs {
                                    Expression::Array(array) => &array.elements,
                                    Expression::LegacyArray(array) => &array.elements,
                                    _ => continue,
                                };
                                for element in elements.iter() {
                                    let ArrayElement::KeyValue(element) = element else { continue };
                                    if array(element.value) {
                                        self.describe(kind, element.key, element.value);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
        if !hooked && !(name.len() > 6 && name.ends_with(b"_theme")) {
            return;
        }
        self.theme_hooks_of(body);
    }

    /// The name a CodeIgniter route is given: the `as` of the options of a
    /// call of one of the route collection's methods,
    /// `$routes->get('books', 'Books::index', ['as' => 'books'])`.
    pub(super) fn named_route(
        &mut self,
        method: &mago_syntax::cst::ClassLikeMemberSelector<'_>,
        arguments: &mago_syntax::cst::ArgumentList<'_>,
    ) {
        const VERBS: [&[u8]; 12] = [
            b"get",
            b"post",
            b"put",
            b"patch",
            b"delete",
            b"options",
            b"head",
            b"add",
            b"cli",
            b"view",
            b"match",
            b"resource",
        ];
        let mago_syntax::cst::ClassLikeMemberSelector::Identifier(name) = method else { return };
        if !VERBS.iter().any(|verb| name.value.eq_ignore_ascii_case(verb)) {
            return;
        }
        for argument in arguments.arguments.iter() {
            let Argument::Positional(argument) = argument else { continue };
            let elements = match argument.value {
                Expression::Array(array) => &array.elements,
                Expression::LegacyArray(array) => &array.elements,
                _ => continue,
            };
            for element in elements.iter() {
                let ArrayElement::KeyValue(element) = element else { continue };
                let named = matches!(element.key, Expression::Literal(Literal::String(key)) if key.value == Some(b"as".as_slice()));
                if named && matches!(element.value, Expression::Literal(Literal::String(_))) {
                    self.declare(DeclaredKind::CodeIgniterRoute, element.value);
                }
            }
        }
    }

    /// Declares the name `key` writes, which the array `value` describes. A
    /// table's columns are declared with it: the keys of its `fields`, each
    /// as `table.column`.
    fn describe(&mut self, kind: DeclaredKind, key: &Expression<'_>, value: &Expression<'_>) {
        let before = self.summary.names.declared.len();
        self.declare(kind, key);
        if kind != DeclaredKind::DrupalTable || self.summary.names.declared.len() == before {
            return;
        }
        let Some(table) = self.summary.names.declared.last().map(|declared| declared.name.clone()) else { return };
        fn elements<'e>(expr: &'e Expression<'e>) -> Option<Vec<&'e ArrayElement<'e>>> {
            match expr {
                Expression::Array(array) => Some(array.elements.iter().collect()),
                Expression::LegacyArray(array) => Some(array.elements.iter().collect()),
                _ => None,
            }
        }
        let is = |expr: &Expression<'_>, wanted: &[u8]| matches!(expr, Expression::Literal(Literal::String(string)) if string.value == Some(wanted));
        let fields = elements(value).into_iter().flatten().find_map(|element| match element {
            ArrayElement::KeyValue(element) if is(element.key, b"fields") => elements(element.value),
            _ => None,
        });
        for field in fields.into_iter().flatten() {
            let ArrayElement::KeyValue(field) = field else { continue };
            let Expression::Literal(Literal::String(column)) = field.key else { continue };
            let Some(column) = column.value.map(|value| String::from_utf8_lossy(value).into_owned()) else { continue };
            let loc = self.loc(inside(field.key));
            self.summary.names.declared.push(DeclaredName {
                kind: DeclaredKind::DrupalColumn,
                name: format!("{table}.{column}"),
                dynamic: false,
                loc,
                params: Vec::new(),
                classes: Vec::new(),
            });
        }
    }

    /// The method `method` of the class being read as an implementation of
    /// the hooks its `#[Hook('name')]` attributes name. Without a method
    /// (the attributes are the class's): `#[Hook('name', method: 'run')]`.
    pub(super) fn hook_methods(&mut self, method: &[u8], lists: &Sequence<'_, AttributeList<'_>>) {
        let Some(class) = self.classes.last().cloned() else { return };
        for attribute in lists.iter().flat_map(|list| list.attributes.iter()) {
            if attribute.name.value().rsplit(|byte| *byte == b'\\').next() != Some(b"Hook".as_slice()) {
                continue;
            }
            let Some(arguments) = attribute.argument_list.as_ref() else { continue };
            let Some(PartialArgument::Positional(argument)) = arguments.arguments.first() else { continue };
            let Expression::Literal(Literal::String(string)) = argument.value else { continue };
            let Some(hook) = string.value.map(|value| String::from_utf8_lossy(value).into_owned()) else { continue };
            // On the class itself: the method the attribute names, else `__invoke`.
            let named = arguments.arguments.iter().skip(1).find_map(|argument| {
                let value = match argument {
                    PartialArgument::Named(argument) if argument.name.value == b"method" => argument.value,
                    PartialArgument::Positional(argument) => argument.value,
                    _ => return None,
                };
                match value {
                    Expression::Literal(Literal::String(string)) => {
                        string.value.map(|value| String::from_utf8_lossy(value).into_owned())
                    }
                    _ => None,
                }
            });
            let method = match method {
                [] => named.filter(|name| !name.is_empty()).unwrap_or_else(|| "__invoke".to_owned()),
                method => String::from_utf8_lossy(method).into_owned(),
            };
            self.summary.names.callbacks.push(crate::summary::HookCallback {
                hook: format!("{}{hook}", crate::summary::DRUPAL_HOOK),
                callback: crate::summary::callback_key(Some(&class), &method),
            });
        }
    }

    fn theme_hooks_of(&mut self, body: &Block<'_>) {
        for statement in body.statements.iter() {
            let Statement::Return(returned) = statement else { continue };
            let elements = match returned.value {
                Some(Expression::Array(array)) => &array.elements,
                Some(Expression::LegacyArray(array)) => &array.elements,
                _ => continue,
            };
            for element in elements.iter() {
                let ArrayElement::KeyValue(element) = element else { continue };
                // A theme hook is described by an array: other functions named so return other things.
                let described = match element.value {
                    Expression::Array(array) => &array.elements,
                    Expression::LegacyArray(array) => &array.elements,
                    _ => continue,
                };
                let before = self.summary.names.declared.len();
                self.declare(DeclaredKind::DrupalThemeHook, element.key);
                if self.summary.names.declared.len() == before {
                    continue;
                }
                // What its template is given: the keys of `variables`, or the one `render element`.
                let mut variables: Vec<String> = Vec::new();
                let string = |expr: &Expression<'_>| match expr {
                    Expression::Literal(Literal::String(string)) => {
                        string.value.map(|value| String::from_utf8_lossy(value).into_owned())
                    }
                    _ => None,
                };
                for entry in described.iter() {
                    let ArrayElement::KeyValue(entry) = entry else { continue };
                    match (string(entry.key).as_deref(), entry.value) {
                        (Some("variables"), Expression::Array(array)) => {
                            variables.extend(array.elements.iter().filter_map(|variable| match variable {
                                ArrayElement::KeyValue(variable) => string(variable.key),
                                _ => None,
                            }));
                        }
                        (Some("render element"), value) => variables.extend(string(value)),
                        _ => {}
                    }
                }
                if let Some(declared) = self.summary.names.declared.last_mut() {
                    declared.classes = variables;
                }
            }
        }
    }
}

/// The first annotation of a docblock with arguments: its name and the text
/// after its `(`.
fn annotation(text: &str) -> Option<(&str, &str)> {
    let mut from = 0;
    while let Some(at) = text[from..].find('@') {
        let rest = &text[from + at + 1..];
        let name =
            rest.split(|c: char| !(c.is_ascii_alphanumeric() || c == '\\' || c == '_')).next().unwrap_or_default();
        if !name.is_empty() && rest[name.len()..].starts_with('(') {
            return Some((name, &rest[name.len() + 1..]));
        }
        from += at + 1;
    }
    None
}

/// The string an annotation's `body` assigns to `key` (`id = "node"`; with
/// `quoted`, `"storage" = "…"`): where it starts in `body`, and what it is.
fn assigned<'t>(body: &'t str, key: &str, quoted: bool) -> Option<(usize, &'t str)> {
    let written = if quoted { format!("\"{key}\"") } else { key.to_owned() };
    let mut from = 0;
    while let Some(at) = body[from..].find(&written) {
        let start = from + at;
        from = start + written.len();
        let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
        if !quoted && body[..start].ends_with(word) {
            continue;
        }
        let Some(value) = body[from..].trim_start().strip_prefix('=') else { continue };
        let value = value.trim_start();
        let Some(value) = value.strip_prefix('"') else { continue };
        let end = value.find('"')?;
        return Some((body.len() - value.len(), &value[..end]));
    }
    None
}

/// CodeIgniter: the events a file fires and the tables its migrations make.
impl Extractor<'_> {
    /// The event `Events::trigger('name', …)` fires.
    pub(super) fn triggered_event(
        &mut self,
        class: &Expression<'_>,
        method: &mago_syntax::cst::ClassLikeMemberSelector<'_>,
        arguments: &mago_syntax::cst::ArgumentList<'_>,
    ) {
        let Expression::Identifier(class) = class else { return };
        let mago_syntax::cst::ClassLikeMemberSelector::Identifier(name) = method else { return };
        let events = class.value().rsplit(|byte| *byte == b'\\').next().is_some_and(|last| last == b"Events");
        if !events || !name.value.eq_ignore_ascii_case(b"trigger") {
            return;
        }
        if let Some(Argument::Positional(first)) = arguments.arguments.first() {
            self.declare(DeclaredKind::CodeIgniterEvent, first.value);
        }
    }

    /// The table a migration makes and its columns: the keys of the arrays
    /// `addField()` is given before `createTable('name')` (or along its
    /// chain), each as `table.column`; and those `addColumn('name', […])`
    /// adds.
    pub(super) fn forge(
        &mut self,
        object: &Expression<'_>,
        method: &mago_syntax::cst::ClassLikeMemberSelector<'_>,
        arguments: &mago_syntax::cst::ArgumentList<'_>,
        end: u32,
    ) {
        let mago_syntax::cst::ClassLikeMemberSelector::Identifier(name) = method else { return };
        fn positional<'e>(
            arguments: &'e mago_syntax::cst::ArgumentList<'e>,
            index: usize,
        ) -> Option<&'e Expression<'e>> {
            match arguments.arguments.iter().nth(index)? {
                Argument::Positional(argument) => Some(argument.value),
                _ => None,
            }
        }
        let Some(first) = positional(arguments, 0) else { return };
        // CodeIgniter 3: `$this->load->model('book_model', 'books')`.
        let library = name.value.eq_ignore_ascii_case(b"library");
        if (library || name.value.eq_ignore_ascii_case(b"model")) && self.source(object.span()).ends_with("->load") {
            let model = match first {
                Expression::Literal(Literal::String(string)) => {
                    string.value.map(|v| String::from_utf8_lossy(v).into_owned())
                }
                _ => None,
            };
            // (A library's name is its third argument: `->library('basket', NULL, 'cart')`.)
            if let (Some(model), Some(alias)) = (model, positional(arguments, if library { 2 } else { 1 })) {
                let before = self.summary.names.declared.len();
                self.declare(DeclaredKind::CodeIgniterLoaded, alias);
                if self.summary.names.declared.len() > before
                    && let Some(declared) = self.summary.names.declared.last_mut()
                {
                    declared.classes = vec![model.rsplit('/').next().unwrap_or(&model).to_owned()];
                }
            }
            return;
        }
        if name.value.eq_ignore_ascii_case(b"addField") || name.value.eq_ignore_ascii_case(b"add_field") {
            if !self.forge_chained.contains(&end) {
                let fields = self.field_keys(first);
                self.forge_fields.extend(fields);
            }
            return;
        }
        let creates =
            name.value.eq_ignore_ascii_case(b"createTable") || name.value.eq_ignore_ascii_case(b"create_table");
        if !creates
            && !(name.value.eq_ignore_ascii_case(b"addColumn") || name.value.eq_ignore_ascii_case(b"add_column"))
        {
            return;
        }
        let mut fields = if creates { std::mem::take(&mut self.forge_fields) } else { Vec::new() };
        if creates {
            // `$forge->addField([...])->addKey('id')->createTable('books')`.
            let mut below = object;
            while let Expression::Call(mago_syntax::cst::Call::Method(call)) = below {
                let added = matches!(&call.method, mago_syntax::cst::ClassLikeMemberSelector::Identifier(name) if (name.value.eq_ignore_ascii_case(b"addField") || name.value.eq_ignore_ascii_case(b"add_field")));
                if added && let Some(array) = positional(&call.argument_list, 0) {
                    fields.extend(self.field_keys(array));
                    self.forge_chained.push(call.span().end.offset);
                }
                below = call.object;
            }
        } else if let Some(array) = positional(arguments, 1) {
            fields = self.field_keys(array);
        }
        let Expression::Literal(Literal::String(string)) = first else { return };
        let Some(table) = string.value.map(|value| String::from_utf8_lossy(value).into_owned()) else { return };
        if table.is_empty() {
            return;
        }
        if creates {
            self.declare(DeclaredKind::CodeIgniterTable, first);
        }
        for (column, loc) in fields {
            self.summary.names.declared.push(DeclaredName {
                kind: DeclaredKind::CodeIgniterColumn,
                name: format!("{table}.{column}"),
                dynamic: false,
                loc,
                params: Vec::new(),
                classes: Vec::new(),
            });
        }
    }

    /// The string keys of the array `expr` whose values are arrays: the
    /// fields of `addField(['id' => ['type' => 'INT'], …])`.
    fn field_keys(&self, expr: &Expression<'_>) -> Vec<(String, crate::summary::Loc)> {
        let elements = match expr {
            Expression::Array(array) => &array.elements,
            Expression::LegacyArray(array) => &array.elements,
            _ => return Vec::new(),
        };
        let mut out = Vec::new();
        for element in elements.iter() {
            let ArrayElement::KeyValue(element) = element else { continue };
            let Expression::Literal(Literal::String(key)) = element.key else { continue };
            if !matches!(element.value, Expression::Array(_) | Expression::LegacyArray(_)) {
                continue;
            }
            if let Some(name) = key.value.map(|value| String::from_utf8_lossy(value).into_owned()) {
                out.push((name, self.loc(inside(element.key))));
            }
        }
        out
    }
}

/// Class aliases: `class_alias()`.
impl Extractor<'_> {
    /// `class_alias('Real', 'Alias')`, with names written as strings or as
    /// `Foo::class`. A name put together at run time declares nothing.
    fn class_alias(&mut self, node: &FunctionCall<'_>) {
        let arguments = node.argument_list.arguments.as_slice();
        let (Some(Argument::Positional(class)), Some(Argument::Positional(alias))) =
            (arguments.first(), arguments.get(1))
        else {
            return;
        };
        if let (Some(class), Some(alias)) = (self.alias_class_name(class.value), self.alias_class_name(alias.value)) {
            self.declare_alias(alias, class);
        }
    }

    /// A class name as an argument of `class_alias()`: a string (its leading
    /// `\` dropped) or `Foo::class`, with the span of the name.
    fn alias_class_name(&self, expr: &Expression<'_>) -> Option<(String, Span)> {
        use mago_syntax::cst::{Access, ClassLikeConstantSelector};
        let (name, span) = match expr {
            Expression::Parenthesized(inner) => return self.alias_class_name(inner.expression),
            Expression::Literal(Literal::String(string)) => {
                (String::from_utf8_lossy(string.value?).into_owned(), inside(expr))
            }
            Expression::Access(Access::ClassConstant(access))
                if matches!(&access.constant, ClassLikeConstantSelector::Identifier(constant)
                    if constant.value.eq_ignore_ascii_case(b"class")) =>
            {
                let Expression::Identifier(class) = access.class else { return None };
                (self.class_name(class), class.span())
            }
            _ => return None,
        };
        let name = name.trim_start_matches('\\');
        let valid = !name.is_empty()
            && !name.ends_with('\\')
            && !name.contains("\\\\")
            && name.chars().all(|c| c == '\\' || c == '_' || c.is_alphanumeric() || !c.is_ascii());
        valid.then(|| (name.to_owned(), span))
    }

    fn declare_alias(&mut self, (alias, span): (String, Span), (class, _): (String, Span)) {
        if alias.eq_ignore_ascii_case(&class) {
            return;
        }
        let loc = self.loc(span);
        self.summary.names.declared.push(DeclaredName {
            kind: DeclaredKind::ClassAlias,
            name: alias,
            dynamic: false,
            loc,
            params: Vec::new(),
            classes: vec![class],
        });
    }
}
