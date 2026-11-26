use super::*;

const REAL: &str = r"<?php
namespace Acme\Orm\Fields;

class ScalarField
{
    public function getName(): string { return ''; }
}

class BooleanField extends ScalarField {}
";

const COMPATIBILITY: &str = r"<?php
namespace Acme\Compat;

class_alias('Acme\Orm\Fields\ScalarField', 'Acme\Entity\ScalarField');
class_alias(\Acme\Orm\Fields\BooleanField::class, '\Acme\Entity\BooleanField');
";

const AUTOLOAD: &str = r"<?php

use Acme\Orm\Fields\ScalarField;

class_alias('\Acme\Orm\Fields\ScalarField', '\Shop\Field');
class_alias(ScalarField::class, 'Legacy_Field');
// An alias of an alias.
class_alias('Legacy_Field', 'Older_Field');
// A loop declares nothing a class lookup can reach.
class_alias('Loop\A', 'Loop\B');
class_alias('Loop\B', 'Loop\A');
// Put together at run time: nothing.
class_alias($class, 'Dynamic\Alias');
";

const USER: &str = r"<?php
namespace Shop\Blocks;

use Acme\Entity;

class DataManager
{
    public function check($field, Entity\ScalarField $typed): void
    {
        if ($field instanceof Entity\ScalarField) {}
        if ($field instanceof Entity\BooleanField) {}
        if ($field instanceof \Shop\Field) {}
        if ($field instanceof \Older_Field) {}
        echo $typed->getName();
    }
}
";

const FIELDS: &str = "lib/orm/fields.php";
const HERE: &str = "lib/blocks/datamanager.php";

fn project() -> Project {
    Project::new(&[
        (FIELDS, REAL),
        ("include/compatibility.php", COMPATIBILITY),
        ("include/autoload.php", AUTOLOAD),
        (HERE, USER),
    ])
}

#[test]
fn an_alias_goes_to_the_class_it_stands_for() {
    let mut project = project();
    let mut at = |from: &str, to: &str| project.definition(HERE, &USER.replacen(from, to, 1));
    let scalar = Some(vec![target(FIELDS, 3, "ScalarField")]);

    // Through a `use`d namespace.
    assert_eq!(at("instanceof Entity\\ScalarField", "instanceof Entity\\Scalar/*|*/Field"), scalar);
    // A class written as `Foo::class`, an alias with a leading `\`.
    assert_eq!(
        at("instanceof Entity\\BooleanField", "instanceof Entity\\Boolean/*|*/Field"),
        Some(vec![target(FIELDS, 8, "BooleanField")])
    );
    // `class_alias()` with leading `\`s.
    assert_eq!(at("\\Shop\\Field", "\\Shop\\Fi/*|*/eld"), scalar);
    // An alias of an alias (of `Foo::class`).
    assert_eq!(at("\\Older_Field", "\\Older_/*|*/Field"), scalar);
}

#[test]
fn an_alias_has_the_members_of_its_class() {
    let mut project = project();
    let found = project.definition(HERE, &USER.replace("$typed->getName()", "$typed->get/*|*/Name()"));
    assert_eq!(found, Some(vec![target(FIELDS, 5, "getName")]));
}

#[test]
fn only_written_aliases_are_declared() {
    let project = project();
    let declares = |fqn: &str| project.engine.declares(NameKind::ClassLike, fqn);
    assert!(declares("Acme\\Entity\\ScalarField"));
    assert!(declares("acme\\entity\\booleanfield"));
    assert!(declares("Shop\\Field"));
    assert!(declares("Older_Field"));
    assert!(!declares("Loop\\A"));
    assert!(!declares("Dynamic\\Alias"));
    assert_eq!(project.engine.class_alias("Loop\\B"), None);
    assert_eq!(project.engine.class_alias("Older_Field").as_deref(), Some("Acme\\Orm\\Fields\\ScalarField"));
}

#[test]
fn a_declared_class_wins_over_an_alias_of_its_name() {
    let mut project = Project::new(&[
        (FIELDS, REAL),
        ("src/alias.php", "<?php\nclass_alias('Acme\\Orm\\Fields\\ScalarField', 'App\\Field');\n"),
        ("src/Field.php", "<?php\nnamespace App;\n\nclass Field {}\n"),
    ]);
    let found = project.definition("src/use.php", "<?php\nnew \\App\\Fi/*|*/eld();\n");
    assert_eq!(found, Some(vec![target("src/Field.php", 3, "Field")]));
}

#[test]
fn a_namespaced_class_alias_function_declares_nothing() {
    let project = Project::new(&[
        (FIELDS, REAL),
        (
            "src/alias.php",
            "<?php\nnamespace Mine;\n\nOwn\\class_alias('Acme\\Orm\\Fields\\ScalarField', 'Mine\\Nope');\n\\class_alias('Acme\\Orm\\Fields\\ScalarField', 'Mine\\Yes');\nclass_alias('Acme\\Orm\\Fields\\ScalarField', 'Mine\\Also');\n",
        ),
    ]);
    assert_eq!(project.engine.class_alias("Mine\\Nope"), None);
    assert!(project.engine.class_alias("Mine\\Yes").is_some());
    assert!(project.engine.class_alias("Mine\\Also").is_some());
}

#[test]
fn an_alias_in_an_open_buffer_counts_and_goes_away_with_it() {
    let mut project = Project::new(&[(FIELDS, REAL), ("src/alias.php", "<?php\n")]);
    assert_eq!(project.engine.class_alias("Fresh\\Alias"), None);
    let doc = project.open("src/alias.php", "<?php\nclass_alias('Acme\\Orm\\Fields\\ScalarField', 'Fresh\\Alias');\n");
    assert_eq!(project.engine.class_alias("Fresh\\Alias").as_deref(), Some("Acme\\Orm\\Fields\\ScalarField"));
    project.engine.document_closed(doc);
    assert_eq!(project.engine.class_alias("Fresh\\Alias"), None);
}

#[test]
fn the_project_s_alias_wins_over_a_library_s() {
    use crate::frameworks::names::ClassAliases;
    use crate::summary::{DeclaredKind, DeclaredName, NameFacts};
    let facts = |class: &str| NameFacts {
        declared: vec![DeclaredName {
            kind: DeclaredKind::ClassAlias,
            name: "Shop\\Field".to_owned(),
            dynamic: false,
            loc: Default::default(),
            params: Vec::new(),
            classes: vec![class.to_owned()],
        }],
        ..NameFacts::default()
    };
    let (library, own, other) = (facts("Vendor\\Field"), facts("Own\\Field"), facts("Other\\Field"));
    let aliases = ClassAliases::new([(&library, true), (&own, false), (&other, false)]);
    assert_eq!(aliases.resolve("\\shop\\field").as_deref(), Some("Own\\Field"));
    let aliases = ClassAliases::new([(&own, false), (&library, true)]);
    assert_eq!(aliases.resolve("Shop\\Field").as_deref(), Some("Own\\Field"));
}
