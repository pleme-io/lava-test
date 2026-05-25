//! Built-in [`Assertion`] impls for the lava domain.
//!
//! Each is a typed struct that implements [`crate::Assertion`]. Operators
//! author tests by composing these (or implementing their own).

use crate::{AssertContext, Assertion, AssertionFailure};

/// `resource.<type_id>.<name>` exists in the rendered terraform.json.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceExists {
    pub type_id: String,
    pub name: String,
}

impl ResourceExists {
    #[must_use]
    pub fn new(type_id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            type_id: type_id.into(),
            name: name.into(),
        }
    }
}

impl Assertion for ResourceExists {
    fn check(&self, ctx: &AssertContext<'_>) -> Result<(), AssertionFailure> {
        let v = ctx
            .terraform_json
            .pointer(&format!("/resource/{}/{}", self.type_id, self.name));
        if v.is_some() {
            Ok(())
        } else {
            Err(AssertionFailure::new(format!(
                "resource `{}.{}` not present",
                self.type_id, self.name
            ))
            .at(format!("/resource/{}/{}", self.type_id, self.name)))
        }
    }

    fn describe(&self) -> String {
        let mut s = String::from("resource-exists ");
        s.push_str(&self.type_id);
        s.push('.');
        s.push_str(&self.name);
        s
    }
}

/// `resource.<type_id>.<name>` does NOT exist in the rendered terraform.json.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoResource {
    pub type_id: String,
    pub name: String,
}

impl NoResource {
    #[must_use]
    pub fn new(type_id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            type_id: type_id.into(),
            name: name.into(),
        }
    }
}

impl Assertion for NoResource {
    fn check(&self, ctx: &AssertContext<'_>) -> Result<(), AssertionFailure> {
        let v = ctx
            .terraform_json
            .pointer(&format!("/resource/{}/{}", self.type_id, self.name));
        if v.is_none() {
            Ok(())
        } else {
            Err(AssertionFailure::new(format!(
                "resource `{}.{}` is present (expected absence)",
                self.type_id, self.name
            ))
            .at(format!("/resource/{}/{}", self.type_id, self.name)))
        }
    }

    fn describe(&self) -> String {
        let mut s = String::from("no-resource ");
        s.push_str(&self.type_id);
        s.push('.');
        s.push_str(&self.name);
        s
    }
}

/// `resource.<type_id>.<name>.<attr>` equals the expected JSON value.
#[derive(Debug, Clone)]
pub struct AttributeEquals {
    pub type_id: String,
    pub name: String,
    pub attr: String,
    pub expected: serde_json::Value,
}

impl AttributeEquals {
    #[must_use]
    pub fn new(
        type_id: impl Into<String>,
        name: impl Into<String>,
        attr: impl Into<String>,
        expected: serde_json::Value,
    ) -> Self {
        Self {
            type_id: type_id.into(),
            name: name.into(),
            attr: attr.into(),
            expected,
        }
    }
}

impl Assertion for AttributeEquals {
    fn check(&self, ctx: &AssertContext<'_>) -> Result<(), AssertionFailure> {
        let pointer = format!("/resource/{}/{}/{}", self.type_id, self.name, self.attr);
        let actual = ctx.terraform_json.pointer(&pointer);
        match actual {
            None => Err(AssertionFailure::new(format!(
                "attribute `{}` not present on `{}.{}`",
                self.attr, self.type_id, self.name
            ))
            .at(pointer)),
            Some(v) if v == &self.expected => Ok(()),
            Some(v) => Err(AssertionFailure::new(format!(
                "expected {} on `{}.{}.{}`, got {}",
                self.expected, self.type_id, self.name, self.attr, v
            ))
            .at(pointer)),
        }
    }

    fn describe(&self) -> String {
        let mut s = String::from("attribute-equals ");
        s.push_str(&self.type_id);
        s.push('.');
        s.push_str(&self.name);
        s.push('.');
        s.push_str(&self.attr);
        s
    }
}

/// Resources of `<type_id>` total exactly N.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceCount {
    pub type_id: String,
    pub expected: usize,
}

impl ResourceCount {
    #[must_use]
    pub fn new(type_id: impl Into<String>, expected: usize) -> Self {
        Self {
            type_id: type_id.into(),
            expected,
        }
    }
}

impl Assertion for ResourceCount {
    fn check(&self, ctx: &AssertContext<'_>) -> Result<(), AssertionFailure> {
        let by_name = ctx
            .terraform_json
            .pointer(&format!("/resource/{}", self.type_id))
            .and_then(serde_json::Value::as_object);
        let n = by_name.map_or(0, serde_json::Map::len);
        if n == self.expected {
            Ok(())
        } else {
            Err(AssertionFailure::new(format!(
                "expected {} resources of type `{}`, got {n}",
                self.expected, self.type_id
            ))
            .at(format!("/resource/{}", self.type_id)))
        }
    }

    fn describe(&self) -> String {
        let mut s = String::from("resource-count ");
        s.push_str(&self.type_id);
        s.push(' ');
        s.push_str(&self.expected.to_string());
        s
    }
}

/// `output.<name>.value` equals the expected JSON value.
#[derive(Debug, Clone)]
pub struct OutputEquals {
    pub name: String,
    pub expected: serde_json::Value,
}

impl OutputEquals {
    #[must_use]
    pub fn new(name: impl Into<String>, expected: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            expected,
        }
    }
}

impl Assertion for OutputEquals {
    fn check(&self, ctx: &AssertContext<'_>) -> Result<(), AssertionFailure> {
        let pointer = format!("/output/{}/value", self.name);
        let actual = ctx.terraform_json.pointer(&pointer);
        match actual {
            None => Err(AssertionFailure::new(format!(
                "output `{}` not present",
                self.name
            ))
            .at(pointer)),
            Some(v) if v == &self.expected => Ok(()),
            Some(v) => Err(AssertionFailure::new(format!(
                "expected output `{}` = {}, got {}",
                self.name, self.expected, v
            ))
            .at(pointer)),
        }
    }

    fn describe(&self) -> String {
        let mut s = String::from("output-equals ");
        s.push_str(&self.name);
        s
    }
}

/// Every `${type.name.attr}` interpolation in the architecture refers
/// to a resource that *exists* in the same architecture. Catches
/// dangling refs at compose time — the value-of typed-ref-graph
/// pattern lava-core already maintains lets us walk this cheaply
/// without re-parsing interpolation strings.
#[derive(Debug, Clone)]
pub struct RefValid;

impl Assertion for RefValid {
    fn check(&self, ctx: &AssertContext<'_>) -> Result<(), AssertionFailure> {
        use lava_core::Value;
        for r in &ctx.architecture.resources {
            for (attr, val) in &r.attributes {
                let mut refs = Vec::new();
                walk_refs(val, &mut refs);
                for rref in refs {
                    let exists = ctx.architecture.resources.iter().any(|target| {
                        target.type_id == rref.type_id && target.name == rref.name
                    });
                    if !exists {
                        return Err(AssertionFailure::new(format!(
                            "`{}.{}.{}` references `{}.{}` which is not declared",
                            r.type_id, r.name, attr, rref.type_id, rref.name
                        ))
                        .at(format!("/resource/{}/{}/{}", r.type_id, r.name, attr)));
                    }
                }
            }
        }
        Ok(())
    }

    fn describe(&self) -> String {
        "ref-valid".to_string()
    }
}

/// `<type>.<name>.tags.<key>` contains the expected value. Useful for
/// asserting a TrustBoundary or Service tag is applied across a fleet
/// of resources.
#[derive(Debug, Clone)]
pub struct TagEquals {
    pub type_id: String,
    pub name: String,
    pub key: String,
    pub expected: String,
}

impl TagEquals {
    #[must_use]
    pub fn new(
        type_id: impl Into<String>,
        name: impl Into<String>,
        key: impl Into<String>,
        expected: impl Into<String>,
    ) -> Self {
        Self {
            type_id: type_id.into(),
            name: name.into(),
            key: key.into(),
            expected: expected.into(),
        }
    }
}

impl Assertion for TagEquals {
    fn check(&self, ctx: &AssertContext<'_>) -> Result<(), AssertionFailure> {
        let pointer = format!(
            "/resource/{}/{}/tags/{}",
            self.type_id, self.name, self.key
        );
        let actual = ctx.terraform_json.pointer(&pointer);
        match actual.and_then(|v| v.as_str()) {
            Some(v) if v == self.expected => Ok(()),
            Some(v) => Err(AssertionFailure::new(format!(
                "tag `{}={}` on `{}.{}` mismatched: got {v}",
                self.key, self.expected, self.type_id, self.name
            ))
            .at(pointer)),
            None => Err(AssertionFailure::new(format!(
                "tag `{}` not set on `{}.{}`",
                self.key, self.type_id, self.name
            ))
            .at(pointer)),
        }
    }
    fn describe(&self) -> String {
        let mut s = String::from("tag-equals ");
        s.push_str(&self.type_id);
        s.push('.');
        s.push_str(&self.name);
        s.push(':');
        s.push_str(&self.key);
        s
    }
}

/// At least N resources of the given type exist. Lower-bound counterpart
/// to ResourceCount (which is exact).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinResourcesOfKind {
    pub type_id: String,
    pub min: usize,
}

impl MinResourcesOfKind {
    #[must_use]
    pub fn new(type_id: impl Into<String>, min: usize) -> Self {
        Self {
            type_id: type_id.into(),
            min,
        }
    }
}

impl Assertion for MinResourcesOfKind {
    fn check(&self, ctx: &AssertContext<'_>) -> Result<(), AssertionFailure> {
        let by_name = ctx
            .terraform_json
            .pointer(&format!("/resource/{}", self.type_id))
            .and_then(serde_json::Value::as_object);
        let n = by_name.map_or(0, serde_json::Map::len);
        if n >= self.min {
            Ok(())
        } else {
            Err(AssertionFailure::new(format!(
                "expected ≥{} resources of `{}`, got {n}",
                self.min, self.type_id
            ))
            .at(format!("/resource/{}", self.type_id)))
        }
    }
    fn describe(&self) -> String {
        let mut s = String::from("min-resources-of-kind ");
        s.push_str(&self.type_id);
        s.push(' ');
        s.push_str(&self.min.to_string());
        s
    }
}

/// `<type>.<name>.<attr>` references the given target resource. Catches
/// wiring-typo regressions where an architecture was meant to reference
/// X but ended up referencing Y.
#[derive(Debug, Clone)]
pub struct RefTargets {
    pub source_type: String,
    pub source_name: String,
    pub source_attr: String,
    pub target_type: String,
    pub target_name: String,
}

impl RefTargets {
    #[must_use]
    pub fn new(
        source_type: impl Into<String>,
        source_name: impl Into<String>,
        source_attr: impl Into<String>,
        target_type: impl Into<String>,
        target_name: impl Into<String>,
    ) -> Self {
        Self {
            source_type: source_type.into(),
            source_name: source_name.into(),
            source_attr: source_attr.into(),
            target_type: target_type.into(),
            target_name: target_name.into(),
        }
    }
}

impl Assertion for RefTargets {
    fn check(&self, ctx: &AssertContext<'_>) -> Result<(), AssertionFailure> {
        let target_resource =
            ctx.architecture.resources.iter().find(|r| {
                r.type_id == self.source_type && r.name == self.source_name
            });
        let Some(resource) = target_resource else {
            return Err(AssertionFailure::new(format!(
                "source resource `{}.{}` not present",
                self.source_type, self.source_name
            ))
            .at(format!(
                "/resource/{}/{}",
                self.source_type, self.source_name
            )));
        };
        let val = resource.attributes.get(&self.source_attr.replace('-', "_"));
        let Some(val) = val else {
            return Err(AssertionFailure::new(format!(
                "source attribute `{}.{}.{}` not set",
                self.source_type, self.source_name, self.source_attr
            )));
        };
        let mut refs = Vec::new();
        walk_refs(val, &mut refs);
        let matched = refs.iter().any(|r| {
            r.type_id == self.target_type && r.name == self.target_name
        });
        if matched {
            Ok(())
        } else {
            Err(AssertionFailure::new(format!(
                "`{}.{}.{}` does not reference `{}.{}`",
                self.source_type,
                self.source_name,
                self.source_attr,
                self.target_type,
                self.target_name
            )))
        }
    }
    fn describe(&self) -> String {
        let mut s = String::from("ref-targets ");
        s.push_str(&self.source_type);
        s.push('.');
        s.push_str(&self.source_name);
        s.push('.');
        s.push_str(&self.source_attr);
        s.push_str(" → ");
        s.push_str(&self.target_type);
        s.push('.');
        s.push_str(&self.target_name);
        s
    }
}

/// Closure-based property assertion. The predicate runs against the
/// terraform.json shape; failure surfaces a typed AssertionFailure
/// with the operator-supplied label.
///
/// Lets advanced test fixtures express assertions that the built-in
/// variants don't cover (e.g. "every aws_subnet's cidr_block falls
/// inside the parent VPC's cidr range").
pub struct PropertyHolds {
    pub label: String,
    pub predicate: Box<dyn Fn(&serde_json::Value) -> Result<(), String> + Send + Sync>,
}

impl std::fmt::Debug for PropertyHolds {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PropertyHolds")
            .field("label", &self.label)
            .finish()
    }
}

impl PropertyHolds {
    #[must_use]
    pub fn new<F>(label: impl Into<String>, predicate: F) -> Self
    where
        F: Fn(&serde_json::Value) -> Result<(), String> + Send + Sync + 'static,
    {
        Self {
            label: label.into(),
            predicate: Box::new(predicate),
        }
    }
}

impl Assertion for PropertyHolds {
    fn check(&self, ctx: &AssertContext<'_>) -> Result<(), AssertionFailure> {
        (self.predicate)(&ctx.terraform_json).map_err(|msg| {
            AssertionFailure::new(format!("`{}` did not hold: {msg}", self.label))
        })
    }
    fn describe(&self) -> String {
        let mut s = String::from("property ");
        s.push_str(&self.label);
        s
    }
}

fn walk_refs(v: &lava_core::Value, out: &mut Vec<lava_core::ResourceRef>) {
    use lava_core::Value;
    match v {
        Value::Ref(r) => out.push(r.clone()),
        Value::Json(json) => walk_json(json, out),
    }
}

fn walk_json(v: &serde_json::Value, out: &mut Vec<lava_core::ResourceRef>) {
    match v {
        serde_json::Value::Array(items) => {
            for item in items {
                walk_json(item, out);
            }
        }
        serde_json::Value::Object(map) => {
            for (_, val) in map {
                walk_json(val, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{run_case_against, TestCase};
    use indexmap::IndexMap;
    use lava_core::{Architecture, Resource, ResourceRef, Value};

    fn tiny_vpc() -> Architecture {
        let mut arch = Architecture::new("net");
        let mut a = IndexMap::new();
        a.insert("cidr_block".to_string(), Value::s("10.0.0.0/16"));
        a.insert("enable_dns_support".to_string(), Value::b(true));
        arch.resources.push(Resource {
            type_id: "aws_vpc".to_string(),
            name: "main".to_string(),
            attributes: a,
            depends_on: vec![],
            provider: None,
            multiplicity: None,
        });
        let mut igw_attrs = IndexMap::new();
        igw_attrs.insert(
            "vpc_id".to_string(),
            Value::Ref(ResourceRef {
                type_id: "aws_vpc".to_string(),
                name: "main".to_string(),
                attribute: "id".to_string(),
            }),
        );
        arch.resources.push(Resource {
            type_id: "aws_internet_gateway".to_string(),
            name: "main".to_string(),
            attributes: igw_attrs,
            depends_on: vec![],
            provider: None,
            multiplicity: None,
        });
        arch.outputs
            .insert("vpc-id".to_string(), Value::s("known-id"));
        arch
    }

    #[test]
    fn resource_exists_passes_for_declared_resource() {
        let arch = tiny_vpc();
        let ctx = AssertContext::from_architecture(&arch).unwrap();
        assert!(ResourceExists::new("aws_vpc", "main").check(&ctx).is_ok());
    }

    #[test]
    fn resource_exists_fails_with_typed_pointer_for_missing_resource() {
        let arch = tiny_vpc();
        let ctx = AssertContext::from_architecture(&arch).unwrap();
        let err = ResourceExists::new("aws_subnet", "nope").check(&ctx).unwrap_err();
        assert!(err.pointer.unwrap().contains("/resource/aws_subnet/nope"));
    }

    #[test]
    fn no_resource_passes_when_absent() {
        let arch = tiny_vpc();
        let ctx = AssertContext::from_architecture(&arch).unwrap();
        assert!(NoResource::new("aws_subnet", "nope").check(&ctx).is_ok());
    }

    #[test]
    fn no_resource_fails_when_present() {
        let arch = tiny_vpc();
        let ctx = AssertContext::from_architecture(&arch).unwrap();
        assert!(NoResource::new("aws_vpc", "main").check(&ctx).is_err());
    }

    #[test]
    fn attribute_equals_passes_for_matching_value() {
        let arch = tiny_vpc();
        let ctx = AssertContext::from_architecture(&arch).unwrap();
        let assertion = AttributeEquals::new(
            "aws_vpc",
            "main",
            "cidr_block",
            serde_json::json!("10.0.0.0/16"),
        );
        assert!(assertion.check(&ctx).is_ok());
    }

    #[test]
    fn attribute_equals_fails_with_diff_pointer() {
        let arch = tiny_vpc();
        let ctx = AssertContext::from_architecture(&arch).unwrap();
        let assertion = AttributeEquals::new(
            "aws_vpc",
            "main",
            "cidr_block",
            serde_json::json!("10.99.0.0/16"),
        );
        let err = assertion.check(&ctx).unwrap_err();
        assert!(err.pointer.unwrap().ends_with("/cidr_block"));
    }

    #[test]
    fn resource_count_matches_exact_n() {
        let arch = tiny_vpc();
        let ctx = AssertContext::from_architecture(&arch).unwrap();
        assert!(ResourceCount::new("aws_vpc", 1).check(&ctx).is_ok());
        assert!(ResourceCount::new("aws_subnet", 0).check(&ctx).is_ok());
        assert!(ResourceCount::new("aws_vpc", 2).check(&ctx).is_err());
    }

    #[test]
    fn output_equals_walks_terraform_output_block() {
        let arch = tiny_vpc();
        let ctx = AssertContext::from_architecture(&arch).unwrap();
        let assertion = OutputEquals::new("vpc-id", serde_json::json!("known-id"));
        assert!(assertion.check(&ctx).is_ok());
    }

    #[test]
    fn ref_valid_passes_when_every_ref_resolves() {
        let arch = tiny_vpc();
        let ctx = AssertContext::from_architecture(&arch).unwrap();
        assert!(RefValid.check(&ctx).is_ok());
    }

    #[test]
    fn ref_valid_fails_when_a_ref_dangles() {
        let mut arch = tiny_vpc();
        // Add a dangling reference to aws_subnet.does_not_exist.
        let mut attrs = IndexMap::new();
        attrs.insert(
            "subnet_id".to_string(),
            Value::Ref(ResourceRef {
                type_id: "aws_subnet".to_string(),
                name: "does_not_exist".to_string(),
                attribute: "id".to_string(),
            }),
        );
        arch.resources.push(Resource {
            type_id: "aws_nat_gateway".to_string(),
            name: "main".to_string(),
            attributes: attrs,
            depends_on: vec![],
            provider: None,
            multiplicity: None,
        });
        let ctx = AssertContext::from_architecture(&arch).unwrap();
        let err = RefValid.check(&ctx).unwrap_err();
        assert!(err.message.contains("aws_subnet.does_not_exist"));
    }

    #[test]
    fn property_holds_runs_closure_against_terraform_json() {
        let arch = tiny_vpc();
        let ctx = AssertContext::from_architecture(&arch).unwrap();
        let ok = PropertyHolds::new("vpc-cidr-present", |json| {
            if json["resource"]["aws_vpc"]["main"]["cidr_block"].is_string() {
                Ok(())
            } else {
                Err("cidr_block missing".into())
            }
        });
        assert!(ok.check(&ctx).is_ok());
    }

    #[test]
    fn property_holds_fails_with_typed_label_on_predicate_error() {
        let arch = tiny_vpc();
        let ctx = AssertContext::from_architecture(&arch).unwrap();
        let nope = PropertyHolds::new("expects-fail", |_| Err("nope".into()));
        let err = nope.check(&ctx).unwrap_err();
        assert!(err.message.contains("expects-fail"));
        assert!(err.message.contains("nope"));
    }

    #[test]
    fn property_holds_describe_carries_label() {
        let p = PropertyHolds::new("my-label", |_| Ok(()));
        assert!(p.describe().contains("my-label"));
    }

    /// One TestCase composing multiple typed assertions through the
    /// runner. Mirrors how operators will compose .test.tlisp fixtures.
    #[test]
    fn run_case_aggregates_pass_and_fail_counts() {
        let arch = tiny_vpc();
        let ctx = AssertContext::from_architecture(&arch).unwrap();
        let case = TestCase {
            name: "vpc smoke".to_string(),
            architecture: Some("net".to_string()),
            bindings: IndexMap::new(),
            assertions: vec![
                Box::new(ResourceExists::new("aws_vpc", "main")),
                Box::new(ResourceExists::new("aws_subnet", "missing")),
                Box::new(AttributeEquals::new(
                    "aws_vpc",
                    "main",
                    "cidr_block",
                    serde_json::json!("10.0.0.0/16"),
                )),
            ],
        };
        let outcome = run_case_against(&case, &ctx);
        assert_eq!(outcome.passed, 2);
        assert_eq!(outcome.failures.len(), 1);
        assert!(!outcome.ok());
    }
}
