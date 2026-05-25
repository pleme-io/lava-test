//! Parse `(deflava-test …)` forms from .tlisp source into typed
//! [`crate::TestCase`] values. Assertion sub-forms dispatch by head
//! symbol; each known head builds the matching typed [`crate::Assertion`]
//! impl.
//!
//! ## Form shape
//!
//! ```lisp
//! (deflava-test aws-vpc-network/default
//!   :architecture aws-vpc-network
//!   :bindings (:name "main" :cidr "10.0.0.0/16")
//!   :assertions ((resource-exists aws-vpc "main-vpc")
//!                (attribute-equals aws-vpc "main-vpc" :cidr-block "10.0.0.0/16")
//!                (resource-count aws-subnet 6)
//!                (output-equals "vpc-id" "main-vpc-id")
//!                (ref-valid)
//!                (no-resource aws-subnet "absent-subnet")))
//! ```
//!
//! ## Supported assertion heads
//!
//! | head               | typed impl                    |
//! |---|---|
//! | `resource-exists`  | [`crate::ResourceExists`]     |
//! | `no-resource`      | [`crate::NoResource`]         |
//! | `attribute-equals` | [`crate::AttributeEquals`]    |
//! | `resource-count`   | [`crate::ResourceCount`]      |
//! | `output-equals`    | [`crate::OutputEquals`]       |
//! | `ref-valid`        | [`crate::RefValid`]           |

use crate::{
    Assertion, AttributeEquals, MinResourcesOfKind, NoResource, OutputEquals, RefTargets, RefValid,
    RegexMatches, ResourceCount, ResourceExists, TagEquals, TestCase,
};
use indexmap::IndexMap;
use lava_eval::{parse_all, Atom, ParseError, Sx};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TestParseError {
    #[error("parse: {0}")]
    Parse(#[from] ParseError),
    #[error("expected deflava-test form")]
    NotTestForm,
    #[error("missing :{0} clause")]
    MissingClause(&'static str),
    #[error("malformed assertion head: {0}")]
    BadAssertion(String),
    #[error("unknown assertion `{0}`")]
    UnknownAssertion(String),
    #[error("assertion `{kind}` needs more arguments: {detail}")]
    InsufficientArgs { kind: String, detail: String },
}

/// Scan a source string for every `(deflava-test …)` form and
/// return one [`TestCase`] per form. Ignores
/// `(deflava-architecture …)` / `(deflava-interface …)` siblings.
///
/// # Errors
/// Surfaces parse errors and per-test conversion errors.
pub fn tests_in_source(src: &str) -> Result<Vec<TestCase>, TestParseError> {
    let forms = parse_all(src)?;
    let mut out = Vec::new();
    for form in forms {
        let Some(xs) = form.as_list() else { continue };
        if xs.first().and_then(Sx::as_sym) == Some("deflava-test") {
            out.push(test_from_form(xs)?);
        }
    }
    Ok(out)
}

/// Convert one `(deflava-test …)` form into a typed [`TestCase`].
///
/// # Errors
/// See [`TestParseError`].
pub fn test_from_form(xs: &[Sx]) -> Result<TestCase, TestParseError> {
    let name = xs
        .get(1)
        .and_then(Sx::as_sym)
        .or_else(|| xs.get(1).and_then(Sx::as_str))
        .ok_or(TestParseError::NotTestForm)?
        .to_string();

    let mut architecture: Option<String> = None;
    let mut bindings: IndexMap<String, String> = IndexMap::new();
    let mut assertions: Vec<Box<dyn Assertion>> = Vec::new();

    let mut i = 2;
    while i + 1 < xs.len() {
        let key = xs[i].as_kw();
        let val = &xs[i + 1];
        match key {
            Some("architecture") => {
                architecture = val
                    .as_sym()
                    .or_else(|| val.as_str())
                    .map(std::string::ToString::to_string);
            }
            Some("bindings") => {
                if let Some(pairs) = val.as_list() {
                    let mut j = 0;
                    while j + 1 < pairs.len() {
                        if let (Some(k), Some(v)) =
                            (pairs[j].as_kw(), pairs[j + 1].as_str())
                        {
                            bindings.insert(k.to_string(), v.to_string());
                        }
                        j += 2;
                    }
                }
            }
            Some("assertions") => {
                let list = val.as_list().ok_or(TestParseError::MissingClause("assertions"))?;
                for a in list {
                    assertions.push(parse_assertion(a)?);
                }
            }
            _ => {}
        }
        i += 2;
    }

    Ok(TestCase {
        name,
        architecture,
        bindings,
        assertions,
    })
}

fn parse_assertion(form: &Sx) -> Result<Box<dyn Assertion>, TestParseError> {
    let xs = form
        .as_list()
        .ok_or_else(|| TestParseError::BadAssertion(format!("not a list: {form:?}")))?;
    let head = xs
        .first()
        .and_then(Sx::as_sym)
        .ok_or_else(|| TestParseError::BadAssertion("head not sym".into()))?;
    match head {
        "resource-exists" => {
            let (type_id, name) = pair_id_name(xs, "resource-exists")?;
            Ok(Box::new(ResourceExists::new(type_id, name)))
        }
        "no-resource" => {
            let (type_id, name) = pair_id_name(xs, "no-resource")?;
            Ok(Box::new(NoResource::new(type_id, name)))
        }
        "attribute-equals" => {
            // (attribute-equals <type-id> "<name>" :<attr> <value>)
            let type_id = xs
                .get(1)
                .and_then(Sx::as_sym)
                .ok_or_else(|| TestParseError::InsufficientArgs {
                    kind: "attribute-equals".into(),
                    detail: "need type-id at position 1".into(),
                })?;
            let name = xs
                .get(2)
                .and_then(Sx::as_str)
                .ok_or_else(|| TestParseError::InsufficientArgs {
                    kind: "attribute-equals".into(),
                    detail: "need \"name\" at position 2".into(),
                })?;
            let attr_kw = xs
                .get(3)
                .and_then(Sx::as_kw)
                .ok_or_else(|| TestParseError::InsufficientArgs {
                    kind: "attribute-equals".into(),
                    detail: "need :attr at position 3".into(),
                })?;
            let value_sx = xs.get(4).ok_or_else(|| TestParseError::InsufficientArgs {
                kind: "attribute-equals".into(),
                detail: "need value at position 4".into(),
            })?;
            let attr = attr_kw.replace('-', "_");
            let value = sx_to_json(value_sx);
            Ok(Box::new(AttributeEquals::new(
                sym_to_type_id(type_id),
                name,
                attr,
                value,
            )))
        }
        "resource-count" => {
            let type_id = xs
                .get(1)
                .and_then(Sx::as_sym)
                .ok_or_else(|| TestParseError::InsufficientArgs {
                    kind: "resource-count".into(),
                    detail: "need type-id at position 1".into(),
                })?;
            let count = xs
                .get(2)
                .and_then(Sx::as_int)
                .ok_or_else(|| TestParseError::InsufficientArgs {
                    kind: "resource-count".into(),
                    detail: "need integer at position 2".into(),
                })?;
            let count = usize::try_from(count).map_err(|_| TestParseError::InsufficientArgs {
                kind: "resource-count".into(),
                detail: "count must be non-negative".into(),
            })?;
            Ok(Box::new(ResourceCount::new(sym_to_type_id(type_id), count)))
        }
        "output-equals" => {
            let name = xs
                .get(1)
                .and_then(Sx::as_str)
                .ok_or_else(|| TestParseError::InsufficientArgs {
                    kind: "output-equals".into(),
                    detail: "need \"name\" at position 1".into(),
                })?;
            let value_sx = xs.get(2).ok_or_else(|| TestParseError::InsufficientArgs {
                kind: "output-equals".into(),
                detail: "need value at position 2".into(),
            })?;
            Ok(Box::new(OutputEquals::new(name, sx_to_json(value_sx))))
        }
        "ref-valid" => Ok(Box::new(RefValid)),
        "tag-equals" => {
            // (tag-equals <type-id> "<name>" :<key> "<value>")
            let type_id = xs
                .get(1)
                .and_then(Sx::as_sym)
                .ok_or_else(|| TestParseError::InsufficientArgs {
                    kind: "tag-equals".into(),
                    detail: "need type-id".into(),
                })?;
            let name = xs
                .get(2)
                .and_then(Sx::as_str)
                .ok_or_else(|| TestParseError::InsufficientArgs {
                    kind: "tag-equals".into(),
                    detail: "need \"name\"".into(),
                })?;
            let key = xs
                .get(3)
                .and_then(Sx::as_kw)
                .ok_or_else(|| TestParseError::InsufficientArgs {
                    kind: "tag-equals".into(),
                    detail: "need :key".into(),
                })?;
            let expected = xs
                .get(4)
                .and_then(Sx::as_str)
                .ok_or_else(|| TestParseError::InsufficientArgs {
                    kind: "tag-equals".into(),
                    detail: "need \"value\"".into(),
                })?;
            Ok(Box::new(TagEquals::new(
                sym_to_type_id(type_id),
                name,
                key,
                expected,
            )))
        }
        "min-resources-of-kind" | "min-resources" => {
            let type_id = xs
                .get(1)
                .and_then(Sx::as_sym)
                .ok_or_else(|| TestParseError::InsufficientArgs {
                    kind: "min-resources-of-kind".into(),
                    detail: "need type-id".into(),
                })?;
            let n = xs
                .get(2)
                .and_then(Sx::as_int)
                .ok_or_else(|| TestParseError::InsufficientArgs {
                    kind: "min-resources-of-kind".into(),
                    detail: "need integer".into(),
                })?;
            let n =
                usize::try_from(n).map_err(|_| TestParseError::InsufficientArgs {
                    kind: "min-resources-of-kind".into(),
                    detail: "non-negative integer".into(),
                })?;
            Ok(Box::new(MinResourcesOfKind::new(sym_to_type_id(type_id), n)))
        }
        "regex-matches" => {
            // (regex-matches <type-id> "<name>" :<attr> "<pattern>")
            let type_id = xs.get(1).and_then(Sx::as_sym).ok_or_else(|| {
                TestParseError::InsufficientArgs {
                    kind: "regex-matches".into(),
                    detail: "need type-id at 1".into(),
                }
            })?;
            let name = xs.get(2).and_then(Sx::as_str).ok_or_else(|| {
                TestParseError::InsufficientArgs {
                    kind: "regex-matches".into(),
                    detail: "need \"name\" at 2".into(),
                }
            })?;
            let attr_kw = xs.get(3).and_then(Sx::as_kw).ok_or_else(|| {
                TestParseError::InsufficientArgs {
                    kind: "regex-matches".into(),
                    detail: "need :attr at 3".into(),
                }
            })?;
            let pattern = xs.get(4).and_then(Sx::as_str).ok_or_else(|| {
                TestParseError::InsufficientArgs {
                    kind: "regex-matches".into(),
                    detail: "need \"pattern\" at 4".into(),
                }
            })?;
            Ok(Box::new(RegexMatches::new(
                sym_to_type_id(type_id),
                name,
                attr_kw.replace('-', "_"),
                pattern,
            )))
        }
        "ref-targets" => {
            // (ref-targets <src-type> "<src-name>" :<src-attr>
            //              <target-type> "<target-name>")
            let src_type = xs
                .get(1)
                .and_then(Sx::as_sym)
                .ok_or_else(|| TestParseError::InsufficientArgs {
                    kind: "ref-targets".into(),
                    detail: "need src-type at 1".into(),
                })?;
            let src_name = xs
                .get(2)
                .and_then(Sx::as_str)
                .ok_or_else(|| TestParseError::InsufficientArgs {
                    kind: "ref-targets".into(),
                    detail: "need \"src-name\" at 2".into(),
                })?;
            let src_attr = xs
                .get(3)
                .and_then(Sx::as_kw)
                .ok_or_else(|| TestParseError::InsufficientArgs {
                    kind: "ref-targets".into(),
                    detail: "need :src-attr at 3".into(),
                })?;
            let tgt_type = xs
                .get(4)
                .and_then(Sx::as_sym)
                .ok_or_else(|| TestParseError::InsufficientArgs {
                    kind: "ref-targets".into(),
                    detail: "need target-type at 4".into(),
                })?;
            let tgt_name = xs
                .get(5)
                .and_then(Sx::as_str)
                .ok_or_else(|| TestParseError::InsufficientArgs {
                    kind: "ref-targets".into(),
                    detail: "need \"target-name\" at 5".into(),
                })?;
            Ok(Box::new(RefTargets::new(
                sym_to_type_id(src_type),
                src_name,
                src_attr,
                sym_to_type_id(tgt_type),
                tgt_name,
            )))
        }
        other => Err(TestParseError::UnknownAssertion(other.to_string())),
    }
}

fn pair_id_name(xs: &[Sx], kind: &str) -> Result<(String, String), TestParseError> {
    let type_id = xs
        .get(1)
        .and_then(Sx::as_sym)
        .ok_or_else(|| TestParseError::InsufficientArgs {
            kind: kind.to_string(),
            detail: "need type-id at position 1".into(),
        })?;
    let name = xs
        .get(2)
        .and_then(Sx::as_str)
        .ok_or_else(|| TestParseError::InsufficientArgs {
            kind: kind.to_string(),
            detail: "need \"name\" at position 2".into(),
        })?;
    Ok((sym_to_type_id(type_id), name.to_string()))
}

fn sym_to_type_id(s: &str) -> String {
    s.replace('-', "_")
}

fn sx_to_json(s: &Sx) -> serde_json::Value {
    match s {
        Sx::Atom(Atom::Str(s) | Atom::Sym(s)) => serde_json::Value::String(s.clone()),
        Sx::Atom(Atom::Int(n)) => serde_json::Value::Number((*n).into()),
        Sx::Atom(Atom::Bool(b)) => serde_json::Value::Bool(*b),
        Sx::Atom(Atom::Kw(s)) => serde_json::Value::String(s.clone()),
        Sx::List(items) => {
            serde_json::Value::Array(items.iter().map(sx_to_json).collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_test_form_with_three_assertions() {
        let src = r#"
            (deflava-test aws-vpc-network/default
              :architecture aws-vpc-network
              :bindings (:name "main")
              :assertions ((resource-exists aws-vpc "main-vpc")
                           (attribute-equals aws-vpc "main-vpc" :cidr-block "10.0.0.0/16")
                           (resource-count aws-subnet 6)))
        "#;
        let cases = tests_in_source(src).unwrap();
        assert_eq!(cases.len(), 1);
        let c = &cases[0];
        assert_eq!(c.name, "aws-vpc-network/default");
        assert_eq!(c.architecture.as_deref(), Some("aws-vpc-network"));
        assert_eq!(c.bindings["name"], "main");
        assert_eq!(c.assertions.len(), 3);
    }

    #[test]
    fn parses_every_built_in_assertion_kind() {
        let src = r#"
            (deflava-test demo/all
              :architecture demo
              :assertions ((resource-exists aws-vpc "main-vpc")
                           (no-resource aws-subnet "absent")
                           (attribute-equals aws-vpc "main-vpc" :enable-dns-support #t)
                           (resource-count aws-subnet 0)
                           (output-equals "vpc-id" "v-12345")
                           (ref-valid)))
        "#;
        let cases = tests_in_source(src).unwrap();
        assert_eq!(cases[0].assertions.len(), 6);
        // Spot-check each describe() carries the kebab head.
        let descriptions: Vec<String> = cases[0]
            .assertions
            .iter()
            .map(|a| a.describe())
            .collect();
        assert!(descriptions[0].starts_with("resource-exists"));
        assert!(descriptions[1].starts_with("no-resource"));
        assert!(descriptions[2].starts_with("attribute-equals"));
        assert!(descriptions[3].starts_with("resource-count"));
        assert!(descriptions[4].starts_with("output-equals"));
        assert!(descriptions[5].starts_with("ref-valid"));
    }

    #[test]
    fn unknown_assertion_head_surfaces_typed_error() {
        let src = r#"
            (deflava-test x
              :assertions ((not-a-real-assertion)))
        "#;
        let err = tests_in_source(src).unwrap_err();
        match err {
            TestParseError::UnknownAssertion(name) => {
                assert_eq!(name, "not-a-real-assertion");
            }
            other => panic!("expected UnknownAssertion, got {other:?}"),
        }
    }

    #[test]
    fn skips_non_test_forms_in_multi_form_source() {
        let src = r#"
            (deflava-interface demo
              :inputs ((:foo :type :string)))
            (deflava-test demo/smoke
              :architecture demo
              :assertions ((resource-exists aws-vpc "main")))
        "#;
        let cases = tests_in_source(src).unwrap();
        assert_eq!(cases.len(), 1);
        assert_eq!(cases[0].name, "demo/smoke");
    }
}
