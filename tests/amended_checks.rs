//! What a module that amends or extends another may assign: a value is
//! checked against the type its base declared, a typed object takes only the
//! properties of its class, and a body is applied once however many modules
//! amend the one that wrote it.
//!
//! Each case is also evaluated through a module that amends it and only sets
//! the renderer, as a host that renders a module does. Expected results are
//! what `pkl eval` 0.32.2 gives.

#![cfg(feature = "native-io")]

use serde_json::json;

const WRAPPER: &str = "amends \"main.pkl\"\noutput { renderer = new JsonRenderer {} }\n";
const WRAPPED_TWICE: &str = "amends \"wrapper.pkl\"\n";

/// Evaluate `main.pkl` directly, through a wrapper that amends it, and
/// through a module that amends the wrapper.
fn eval_layers(files: &[(&str, &str)]) -> [pklr::Result<serde_json::Value>; 3] {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("pklr-amended-checks-{}-{id}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, source) in files {
        std::fs::write(dir.join(name), source).unwrap();
    }
    std::fs::write(dir.join("wrapper.pkl"), WRAPPER).unwrap();
    std::fs::write(dir.join("wrapped_twice.pkl"), WRAPPED_TWICE).unwrap();
    let results = ["main.pkl", "wrapper.pkl", "wrapped_twice.pkl"]
        .map(|name| pklr::eval_to_json(&dir.join(name)));
    let _ = std::fs::remove_dir_all(&dir);
    results
}

/// The value of `main.pkl`, which every layer must give.
fn eval(files: &[(&str, &str)]) -> serde_json::Value {
    let [direct, wrapped, wrapped_twice] =
        eval_layers(files).map(|result| result.expect("evaluation failed"));
    assert_eq!(direct, wrapped, "through a wrapper");
    assert_eq!(direct, wrapped_twice, "through two wrappers");
    direct
}

/// The error of `main.pkl`, which every layer must fail with.
fn eval_fails(files: &[(&str, &str)]) -> String {
    let [direct, wrapped, wrapped_twice] = eval_layers(files).map(|result| match result {
        Err(error) => error.to_string(),
        Ok(value) => panic!("expected an error, got {value}"),
    });
    assert_eq!(direct, wrapped, "through a wrapper");
    assert_eq!(direct, wrapped_twice, "through two wrappers");
    direct
}

fn main_over(base: &'static str, main: &'static str) -> [(&'static str, &'static str); 2] {
    [("base.pkl", base), ("main.pkl", main)]
}

// ---- a value is checked against the type the base declared ----

const TYPED: &str = r##"open module base
typealias Kit = String(matches(Regex(#"[a-z]+@[0-9]+"#)))
r: Int(isBetween(1, 10)) = 5
kit: String(matches(Regex(#"[a-z]+@[0-9]+"#))) = "a@1"
aliased: Kit = "a@1"
maybe: Int(isPositive)? = null
limit: Int = 10
below: Int(this < limit) = 1
plain: String = "s"
"##;

#[test]
fn amended_value_must_satisfy_the_inherited_constraint() {
    for (main, expected) in [
        (
            "amends \"base.pkl\"\nr = 20\n",
            "Type constraint `isBetween(1, 10)` violated.",
        ),
        (
            "extends \"base.pkl\"\nr = 20\n",
            "Type constraint `isBetween(1, 10)` violated.",
        ),
        (
            "amends \"base.pkl\"\nr = limit * 2\n",
            "Type constraint `isBetween(1, 10)` violated.",
        ),
        (
            "amends \"base.pkl\"\nr = \"x\"\n",
            "Expected value of type `Int`, but got type `String`.",
        ),
        (
            "extends \"base.pkl\"\nr = \"x\"\n",
            "Expected value of type `Int`, but got type `String`.",
        ),
        (
            "amends \"base.pkl\"\nkit = \"nope\"\n",
            "Type constraint `matches(Regex(#\"[a-z]+@[0-9]+\"#))` violated.",
        ),
        (
            "amends \"base.pkl\"\naliased = \"nope\"\n",
            "Type constraint `matches(Regex(#\"[a-z]+@[0-9]+\"#))` violated.",
        ),
        (
            "amends \"base.pkl\"\nmaybe = -3\n",
            "Type constraint `isPositive` violated.",
        ),
        (
            "amends \"base.pkl\"\nbelow = 20\n",
            "Type constraint `this < limit` violated.",
        ),
        // The constraint of a property the module leaves alone reads one it
        // overrides.
        (
            "amends \"base.pkl\"\nlimit = 0\n",
            "Type constraint `this < limit` violated.",
        ),
        (
            "amends \"base.pkl\"\nplain = 1\n",
            "Expected value of type `String`, but got type `Int`.",
        ),
    ] {
        let files = [("base.pkl", TYPED), ("main.pkl", main)];
        let error = eval_fails(&files);
        assert!(error.contains(expected), "{main}: {error}");
    }
}

#[test]
fn amended_value_that_satisfies_the_inherited_type_is_kept() {
    let value = eval(&main_over(
        TYPED,
        "amends \"base.pkl\"\nr = 7\nkit = \"abc@12\"\naliased = \"b@2\"\nmaybe = 3\nlimit = 100\nbelow = 50\n",
    ));
    assert_eq!(
        value,
        json!({
            "r": 7, "kit": "abc@12", "aliased": "b@2", "maybe": 3,
            "limit": 100, "below": 50, "plain": "s"
        })
    );
}

#[test]
fn inherited_constraint_holds_through_a_chain_of_amending_modules() {
    let mid = "amends \"base.pkl\"\nr = 7\n";
    let error = eval_fails(&[
        ("base.pkl", TYPED),
        ("mid.pkl", mid),
        ("main.pkl", "amends \"mid.pkl\"\nr = 20\n"),
    ]);
    assert!(
        error.contains("Type constraint `isBetween(1, 10)` violated."),
        "{error}"
    );

    let value = eval(&[
        ("base.pkl", TYPED),
        ("mid.pkl", mid),
        ("main.pkl", "amends \"mid.pkl\"\nr = 2\n"),
    ]);
    assert_eq!(value["r"], 2);
}

#[test]
fn failed_amended_value_fails_only_the_module_that_renders_it() {
    let bad = "amends \"base.pkl\"\nr = 20\n";
    let value = eval(&[
        ("base.pkl", TYPED),
        ("bad.pkl", bad),
        ("main.pkl", "import \"bad.pkl\"\nv = bad.plain\n"),
    ]);
    assert_eq!(value, json!({"v": "s"}));

    let error = eval_fails(&[
        ("base.pkl", TYPED),
        ("bad.pkl", bad),
        ("main.pkl", "import \"bad.pkl\"\nv = bad.r\n"),
    ]);
    assert!(
        error.contains("Type constraint `isBetween(1, 10)` violated."),
        "{error}"
    );

    // A module that amends the failed one without replacing the value fails
    // too; one that replaces it does not.
    let error = eval_fails(&[
        ("base.pkl", TYPED),
        ("bad.pkl", bad),
        ("main.pkl", "amends \"bad.pkl\"\nplain = \"t\"\n"),
    ]);
    assert!(
        error.contains("Type constraint `isBetween(1, 10)` violated."),
        "{error}"
    );
    let value = eval(&[
        ("base.pkl", TYPED),
        ("bad.pkl", bad),
        ("main.pkl", "amends \"bad.pkl\"\nr = 3\n"),
    ]);
    assert_eq!(value["r"], 3);
}

/// A template module may declare defaults that do not satisfy their
/// constraints until a module that amends it sets them: here exactly one of
/// `a` and `b` must be set. A module that extends the template without
/// setting either is still usable as the value type of a Mapping, and fails
/// only where its own default is rendered.
#[test]
fn unassigned_default_of_a_template_is_judged_only_where_it_is_rendered() {
    const UTIL: &str = "function isExclusive(a: Any, b: Any) =\n  \
        if (a != null) b == null else if (b != null) a == null else false\n";
    const TEMPLATE: &str = "open module template\nimport \"util.pkl\"\n\
        a: String?(util.isExclusive(this, b))\nb: String?\nother: Int = 1\n";
    const JOB: &str = "extends \"template.pkl\"\nother = 2\n";
    const HOLDER: &str = "import \"job.pkl\"\nm: Mapping<String, job>\n";
    let shared = [
        ("util.pkl", UTIL),
        ("template.pkl", TEMPLATE),
        ("job.pkl", JOB),
        ("holder.pkl", HOLDER),
    ];
    let with_main = |main: &'static str| {
        let mut files = shared.to_vec();
        files.push(("main.pkl", main));
        files
    };

    let value = eval(&with_main(
        "amends \"holder.pkl\"\nm { [\"x\"] { a = \"set\" } }\n",
    ));
    assert_eq!(value, json!({"m": {"x": {"a": "set", "other": 2}}}));

    let value = eval(&with_main("amends \"job.pkl\"\nb = \"y\"\n"));
    assert_eq!(value, json!({"b": "y", "other": 2}));

    let error = eval_fails(&with_main("amends \"job.pkl\"\nother = 5\n"));
    assert!(
        error.contains("Type constraint `util.isExclusive(this, b)` violated."),
        "{error}"
    );
}

// ---- a typed object takes only the properties of its class ----

const CLASSES: &str = r#"open module base
class N { also: Listing<String>; approve: Boolean = false }
class Outer { inner: N; many: Listing<N>; byName: Mapping<String, N> }
typealias NA = N
m: Mapping<String, N>
n: N
na: NA
l: Listing<N>
o: Outer
d: Dynamic
"#;

#[test]
fn unknown_property_of_a_typed_object_is_refused() {
    for (main, class) in [
        (
            "amends \"base.pkl\"\nm { [\"a\"] = new { aprove = true } }\n",
            "base#N",
        ),
        (
            "amends \"base.pkl\"\nm { [\"a\"] { aprove = true } }\n",
            "base#N",
        ),
        (
            "extends \"base.pkl\"\nm { [\"a\"] { aprove = true } }\n",
            "base#N",
        ),
        (
            "amends \"base.pkl\"\nm { default { aprove = true }; [\"a\"] {} }\n",
            "base#N",
        ),
        ("amends \"base.pkl\"\nn { aprove = true }\n", "base#N"),
        ("extends \"base.pkl\"\nn { aprove = true }\n", "base#N"),
        ("amends \"base.pkl\"\nn = new { aprove = true }\n", "base#N"),
        ("amends \"base.pkl\"\nna { aprove = true }\n", "base#N"),
        (
            "amends \"base.pkl\"\nl { default { aprove = true }; new {} }\n",
            "base#N",
        ),
        (
            "amends \"base.pkl\"\no { byName { [\"k\"] { aprove = true } } }\n",
            "base#N",
        ),
        (
            "amends \"base.pkl\"\no { innr { approve = true } }\n",
            "base#Outer",
        ),
    ] {
        let error = eval_fails(&main_over(CLASSES, main));
        assert!(
            error.contains(&format!(
                "Cannot find property `aprove` in object of type `{class}`."
            )) || error.contains(&format!(
                "Cannot find property `innr` in object of type `{class}`."
            )),
            "{main}: {error}"
        );
    }
}

#[test]
fn unknown_property_is_refused_in_the_module_that_declares_the_class() {
    for main in [
        "class N { approve: Boolean = false }\nm: Mapping<String, N> = new { [\"a\"] { aprove = true } }\n",
        "class N { approve: Boolean = false }\nm: Mapping<String, N> = new { [\"a\"] = new { aprove = true } }\n",
        "class N { approve: Boolean = false }\nn: N = new { approve = true }\nn2 = (n) { aprove = true }\n",
    ] {
        let error = eval_fails(&[("main.pkl", main)]);
        assert!(
            error.contains("Cannot find property `aprove` in object of type `main#N`."),
            "{main}: {error}"
        );
    }
}

#[test]
fn unknown_property_is_refused_through_a_chain_of_amending_modules() {
    let mid = "amends \"base.pkl\"\nm { [\"a\"] { approve = true } }\nn { approve = true }\n";
    for main in [
        "amends \"mid.pkl\"\nm { [\"a\"] { aprove = true } }\n",
        "amends \"mid.pkl\"\nn { aprove = true }\n",
    ] {
        let error = eval_fails(&[("base.pkl", CLASSES), ("mid.pkl", mid), ("main.pkl", main)]);
        assert!(
            error.contains("Cannot find property `aprove` in object of type `base#N`."),
            "{main}: {error}"
        );
    }
}

#[test]
fn declared_properties_of_a_typed_object_are_accepted() {
    let value = eval(&main_over(
        CLASSES,
        "amends \"base.pkl\"\n\
         m { [\"a\"] { approve = true }; [\"b\"] = new { approve = true }; [\"c\"] { local z = true; approve = z } }\n\
         n { approve = true }\n\
         na { approve = true }\n\
         l { new { approve = true } }\n\
         o { inner { approve = true }; byName { [\"k\"] { approve = true } } }\n\
         d { anything = true }\n",
    ));
    let on = json!({"also": [], "approve": true});
    assert_eq!(
        value,
        json!({
            "m": {"a": on, "b": on, "c": on},
            "n": on,
            "na": on,
            "l": [on],
            "o": {"inner": on, "many": [], "byName": {"k": on}},
            "d": {"anything": true}
        })
    );
}

#[test]
fn subclass_properties_are_accepted_where_the_parent_class_is_declared() {
    let base = "open module base\n\
        abstract class Shape { name: String }\n\
        class Circle extends Shape { r: Float = 1.0 }\n\
        byName: Mapping<String, Shape>\n\
        circles: Mapping<String, Circle>\n";
    let value = eval(&main_over(
        base,
        "amends \"base.pkl\"\n\
         byName { [\"k\"] = new Circle { name = \"k\"; r = 2.0 } }\n\
         circles { [\"a\"] { name = \"a\"; r = 3.0 } }\n",
    ));
    assert_eq!(
        value,
        json!({
            "byName": {"k": {"name": "k", "r": 2.0}},
            "circles": {"a": {"name": "a", "r": 3.0}}
        })
    );
    let error = eval_fails(&main_over(
        base,
        "amends \"base.pkl\"\ncircles { [\"a\"] { name = \"a\"; w = 2 } }\n",
    ));
    assert!(
        error.contains("Cannot find property `w` in object of type `base#Circle`."),
        "{error}"
    );
}

// ---- a body is applied once ----

#[test]
fn nested_listing_is_not_repeated_when_the_module_is_amended_again() {
    let on = |also: serde_json::Value| json!({"also": also, "approve": false});
    for main in [
        "amends \"base.pkl\"\nm { [\"a\"] { also { \"x\"; \"y\" } } }\n",
        "extends \"base.pkl\"\nm { [\"a\"] { also { \"x\"; \"y\" } } }\n",
        "amends \"base.pkl\"\nm { [\"a\"] = new { also { \"x\"; \"y\" } } }\n",
    ] {
        let value = eval(&main_over(CLASSES, main));
        assert_eq!(value["m"], json!({"a": on(json!(["x", "y"]))}), "{main}");
    }

    let value = eval(&main_over(
        CLASSES,
        "amends \"base.pkl\"\n\
         n { also { \"x\" } }\n\
         l { new { also { \"x\" } } }\n\
         o { inner { also { \"x\" } }; many { new { also { \"q\" } } }; byName { [\"k\"] { also { \"z\" } } } }\n\
         d { items { 1; 2 } }\n",
    ));
    assert_eq!(value["n"], on(json!(["x"])));
    assert_eq!(value["l"], json!([on(json!(["x"]))]));
    assert_eq!(
        value["o"],
        json!({
            "inner": on(json!(["x"])),
            "many": [on(json!(["q"]))],
            "byName": {"k": on(json!(["z"]))}
        })
    );
    assert_eq!(value["d"], json!({"items": [1, 2]}));
}

#[test]
fn nested_listing_reading_the_module_is_built_once_and_late_bound() {
    let base = "open module base\n\
        class N { also: Listing<String> }\n\
        name: String = \"base\"\n\
        m: Mapping<String, N>\n";
    let mid = "amends \"base.pkl\"\nm { [\"a\"] { also { module.name; \"y\" } } }\n";
    let value = eval(&[("base.pkl", base), ("main.pkl", mid)]);
    assert_eq!(value["m"], json!({"a": {"also": ["base", "y"]}}));

    let value = eval(&[
        ("base.pkl", base),
        ("mid.pkl", mid),
        ("main.pkl", "amends \"mid.pkl\"\nname = \"top\"\n"),
    ]);
    assert_eq!(value["m"], json!({"a": {"also": ["top", "y"]}}));
}

#[test]
fn each_amending_module_adds_its_own_elements_once() {
    let value = eval(&[
        ("base.pkl", CLASSES),
        (
            "mid.pkl",
            "amends \"base.pkl\"\nm { [\"a\"] { also { \"x\" } } }\nn { also { \"x\" } }\n",
        ),
        (
            "main.pkl",
            "amends \"mid.pkl\"\n\
             m { [\"a\"] { also { \"more\" } }; [\"c\"] { also { \"c\" } } }\n\
             n { also { \"more\" } }\n",
        ),
    ]);
    assert_eq!(
        value["m"],
        json!({
            "a": {"also": ["x", "more"], "approve": false},
            "c": {"also": ["c"], "approve": false}
        })
    );
    assert_eq!(value["n"], json!({"also": ["x", "more"], "approve": false}));
}
