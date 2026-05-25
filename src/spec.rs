//! BDD-style `(deflava-spec …)` parser. A spec is sugar over the
//! existing [`crate::TestCase`] surface — each `:scenarios` clause
//! becomes one TestCase shape so the existing runner consumes specs
//! and tests through the same machinery.
//!
//! ## Form
//!
//! ```lisp
//! (deflava-spec aws-vpc-network
//!   :scenarios (
//!     (:name "default-cidr gives 6 subnets"
//!      :given (:architecture aws-vpc-network)
//!      :when  (:bindings ())
//!      :then  ((resource-count aws-subnet 6)
//!              (attribute-equals aws-vpc "main-vpc" :cidr-block "10.0.0.0/16")))
//!     (:name "override propagates"
//!      :given (:architecture aws-vpc-network)
//!      :when  (:bindings (:name "preview"))
//!      :then  ((resource-exists aws-vpc "preview-vpc")
//!              (no-resource aws-vpc "main-vpc")))))
//! ```
//!
//! Each scenario produces a [`crate::TestCase`] named
//! `<spec-name>/<scenario-name-as-kebab>` and runs through the same
//! `run_case_against` path.

use crate::{parser::test_from_form, TestCase, TestParseError};
use indexmap::IndexMap;
use lava_eval::{parse_all, Sx};

/// Scan a source string for every `(deflava-spec …)` form and return
/// one [`TestCase`] per scenario the spec declares.
///
/// # Errors
/// Surfaces parse errors and per-scenario conversion errors.
pub fn scenarios_in_source(src: &str) -> Result<Vec<TestCase>, TestParseError> {
    let forms = parse_all(src)?;
    let mut out = Vec::new();
    for form in forms {
        let Some(xs) = form.as_list() else { continue };
        if xs.first().and_then(Sx::as_sym) == Some("deflava-spec") {
            let cases = spec_to_cases(xs)?;
            out.extend(cases);
        }
    }
    Ok(out)
}

fn spec_to_cases(xs: &[Sx]) -> Result<Vec<TestCase>, TestParseError> {
    let spec_name = xs
        .get(1)
        .and_then(Sx::as_sym)
        .or_else(|| xs.get(1).and_then(Sx::as_str))
        .ok_or(TestParseError::NotTestForm)?
        .to_string();

    // Walk top-level keyword clauses; we care about :scenarios.
    let mut scenarios: Option<&Sx> = None;
    let mut i = 2;
    while i + 1 < xs.len() {
        if xs[i].as_kw() == Some("scenarios") {
            scenarios = Some(&xs[i + 1]);
        }
        i += 2;
    }
    let scenarios =
        scenarios.ok_or(TestParseError::MissingClause("scenarios"))?;
    let list = scenarios
        .as_list()
        .ok_or(TestParseError::MissingClause("scenarios"))?;

    let mut cases = Vec::with_capacity(list.len());
    for scenario in list {
        cases.push(scenario_to_case(&spec_name, scenario)?);
    }
    Ok(cases)
}

fn scenario_to_case(spec_name: &str, scenario: &Sx) -> Result<TestCase, TestParseError> {
    let xs = scenario
        .as_list()
        .ok_or_else(|| TestParseError::BadAssertion("scenario not a list".into()))?;

    // Walk :keyword clauses on the scenario.
    let mut name: Option<String> = None;
    let mut given: Option<&Sx> = None;
    let mut when: Option<&Sx> = None;
    let mut then: Option<&Sx> = None;

    let mut i = 0;
    while i + 1 < xs.len() {
        match xs[i].as_kw() {
            Some("name") => {
                name = xs[i + 1]
                    .as_str()
                    .map(std::string::ToString::to_string);
            }
            Some("given") => given = Some(&xs[i + 1]),
            Some("when") => when = Some(&xs[i + 1]),
            Some("then") => then = Some(&xs[i + 1]),
            _ => {}
        }
        i += 2;
    }

    let scenario_name = name.unwrap_or_else(|| format!("scenario-{}", spec_name));
    let architecture = given
        .and_then(extract_keyword_target)
        .or_else(|| Some(spec_name.to_string()));
    let bindings = when.map(extract_bindings).unwrap_or_default();
    let then_list = then
        .and_then(Sx::as_list)
        .ok_or(TestParseError::MissingClause("then"))?;

    // Build the synthetic (deflava-test …) form and reuse the existing
    // parser → typed TestCase pipeline.
    let synth_name = format!("{spec_name}/{}", scenario_name_to_slug(&scenario_name));
    let case = synth_test_case(
        &synth_name,
        architecture.as_deref().unwrap_or(spec_name),
        &bindings,
        then_list,
    )?;
    Ok(case)
}

fn extract_keyword_target(form: &Sx) -> Option<String> {
    let xs = form.as_list()?;
    let mut i = 0;
    while i + 1 < xs.len() {
        if xs[i].as_kw() == Some("architecture") {
            return xs[i + 1]
                .as_sym()
                .or_else(|| xs[i + 1].as_str())
                .map(std::string::ToString::to_string);
        }
        i += 2;
    }
    None
}

fn extract_bindings(form: &Sx) -> IndexMap<String, String> {
    let mut out: IndexMap<String, String> = IndexMap::new();
    let Some(xs) = form.as_list() else { return out };
    let mut i = 0;
    while i + 1 < xs.len() {
        if xs[i].as_kw() == Some("bindings") {
            if let Some(pairs) = xs[i + 1].as_list() {
                let mut j = 0;
                while j + 1 < pairs.len() {
                    if let (Some(k), Some(v)) =
                        (pairs[j].as_kw(), pairs[j + 1].as_str())
                    {
                        out.insert(k.to_string(), v.to_string());
                    }
                    j += 2;
                }
            }
        }
        i += 2;
    }
    out
}

fn scenario_name_to_slug(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut prev_dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_end_matches('-').to_string()
}

/// Build a typed TestCase by routing through the existing parser —
/// keeps the spec → test path a thin sugar without re-implementing
/// assertion dispatch.
fn synth_test_case(
    case_name: &str,
    architecture: &str,
    bindings: &IndexMap<String, String>,
    then_list: &[Sx],
) -> Result<TestCase, TestParseError> {
    // We assemble a (deflava-test …) form as parsed Sx values, NOT as
    // source code (no format!() of tlisp). Each :bindings entry is a
    // (:key value) keyword/value pair; :assertions is the original
    // then_list cloned through.
    use lava_eval::{Atom, Sx};
    let mut form: Vec<Sx> = Vec::with_capacity(8);
    form.push(Sx::Atom(Atom::Sym("deflava-test".into())));
    form.push(Sx::Atom(Atom::Str(case_name.into())));
    form.push(Sx::Atom(Atom::Kw("architecture".into())));
    form.push(Sx::Atom(Atom::Sym(architecture.into())));
    // :bindings (:k v :k v ...)
    if !bindings.is_empty() {
        let mut pairs: Vec<Sx> = Vec::with_capacity(bindings.len() * 2);
        for (k, v) in bindings {
            pairs.push(Sx::Atom(Atom::Kw(k.clone())));
            pairs.push(Sx::Atom(Atom::Str(v.clone())));
        }
        form.push(Sx::Atom(Atom::Kw("bindings".into())));
        form.push(Sx::List(pairs));
    }
    // :assertions (...)
    form.push(Sx::Atom(Atom::Kw("assertions".into())));
    form.push(Sx::List(then_list.to_vec()));
    test_from_form(&form)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_with_two_scenarios_produces_two_typed_test_cases() {
        let src = r#"
            (deflava-spec aws-vpc-network
              :scenarios (
                (:name "default-cidr"
                 :given (:architecture aws-vpc-network)
                 :when  (:bindings ())
                 :then  ((resource-count aws-subnet 6)))
                (:name "override propagates"
                 :given (:architecture aws-vpc-network)
                 :when  (:bindings (:name "preview"))
                 :then  ((resource-exists aws-vpc "preview-vpc")
                         (no-resource aws-vpc "main-vpc")))))
        "#;
        let cases = scenarios_in_source(src).unwrap();
        assert_eq!(cases.len(), 2);
        assert_eq!(cases[0].name, "aws-vpc-network/default-cidr");
        assert_eq!(cases[0].architecture.as_deref(), Some("aws-vpc-network"));
        assert_eq!(cases[0].assertions.len(), 1);
        assert_eq!(cases[1].name, "aws-vpc-network/override-propagates");
        assert_eq!(cases[1].bindings["name"], "preview");
        assert_eq!(cases[1].assertions.len(), 2);
    }

    #[test]
    fn spec_falls_back_to_spec_name_for_architecture_when_omitted() {
        let src = r#"
            (deflava-spec demo
              :scenarios ((:name "smoke" :then ((resource-exists aws-vpc "main")))))
        "#;
        let cases = scenarios_in_source(src).unwrap();
        assert_eq!(cases[0].architecture.as_deref(), Some("demo"));
    }

    #[test]
    fn scenario_name_slug_is_kebab() {
        assert_eq!(
            scenario_name_to_slug("Default cidr gives 6 subnets!"),
            "default-cidr-gives-6-subnets"
        );
    }

    #[test]
    fn spec_missing_scenarios_clause_surfaces_typed_error() {
        let src = r#"(deflava-spec demo)"#;
        let err = scenarios_in_source(src).unwrap_err();
        match err {
            TestParseError::MissingClause("scenarios") => {}
            other => panic!("expected MissingClause(scenarios), got {other:?}"),
        }
    }

    #[test]
    fn skips_non_spec_forms_in_multi_form_source() {
        let src = r#"
            (deflava-interface demo :inputs ((:foo :type :string)))
            (deflava-spec demo
              :scenarios ((:name "smoke"
                           :given (:architecture demo)
                           :then ((resource-exists aws-vpc "main")))))
        "#;
        let cases = scenarios_in_source(src).unwrap();
        assert_eq!(cases.len(), 1);
    }
}
