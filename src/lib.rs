//! lava-test — typed TDD/BDD framework for the lava IaC stack and
//! the wider tatara-lisp ecosystem.
//!
//! ## Shape
//!
//! ```text
//! TestSuite { cases: Vec<TestCase> }
//!   TestCase { name, architecture?, bindings, assertions: Vec<Box<dyn Assertion>> }
//!     Assertion (trait): check(&AssertContext) -> Result<(), AssertionFailure>
//!       AssertContext { architecture, terraform_json }
//!
//! run_suite(&suite) → TestReport { passed, failed, failures: Vec<AssertionFailure> }
//! ```
//!
//! ## Built-in lava assertion variants
//!
//! | Variant                  | Checks                                                 |
//! |---|---|
//! | [`ResourceExists`]       | A `<type_id>.<name>` resource is in the rendered shape |
//! | [`NoResource`]           | A `<type_id>.<name>` is *not* present                  |
//! | [`AttributeEquals`]      | `resource.<type>.<name>.<attr> == <expected>`          |
//! | [`ResourceCount`]        | Resources of `<type_id>` total exactly N               |
//! | [`OutputEquals`]         | `output.<name>.value == <expected>`                    |
//! | [`RefValid`]             | A `${type.name.attr}` interpolation has a matching     |
//! |                          | resource declaration in the same architecture          |
//!
//! ## Extending
//!
//! Implement [`Assertion`] for any custom check. The runner dispatches
//! on the trait object — no built-in variant is privileged. Other
//! tatara-lisp consumers (caixa SDLC, future Lisp DSLs) implement
//! `Assertion` for their own domain (e.g. "every caixa.lisp ships a
//! flake.nix output", "every CRD spec validates against its schema").

#![allow(clippy::module_name_repetitions)]

use indexmap::IndexMap;
use lava_core::Architecture;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod assertions;
pub use assertions::{
    AttributeEquals, NoResource, OutputEquals, RefValid, ResourceCount, ResourceExists,
};

/// Context every assertion sees. Holds the architecture + its rendered
/// terraform.json (lazily memoized so multiple assertions don't re-render).
pub struct AssertContext<'a> {
    pub architecture: &'a Architecture,
    pub terraform_json: serde_json::Value,
}

impl<'a> AssertContext<'a> {
    /// Build a context from an architecture, eagerly rendering its
    /// terraform.json. Cheap (renderer is O(n) over resources).
    ///
    /// # Errors
    /// Surfaces [`AssertionFailure::Render`] when the architecture
    /// refuses to render.
    pub fn from_architecture(arch: &'a Architecture) -> Result<Self, AssertionFailure> {
        let terraform_json = arch
            .render_terraform_json()
            .map_err(|e| AssertionFailure::render(format!("render: {e}")))?;
        Ok(Self {
            architecture: arch,
            terraform_json,
        })
    }
}

/// Typed assertion trait. Implementations describe a single check;
/// the runner walks every assertion in a TestCase and aggregates the
/// failures.
pub trait Assertion: std::fmt::Debug {
    /// Run the check. `Ok(())` = passed; `Err(AssertionFailure)` =
    /// reported in the test report with the assertion's typed kind.
    ///
    /// # Errors
    /// Surfaces a typed [`AssertionFailure`] when the check doesn't
    /// hold.
    fn check(&self, ctx: &AssertContext<'_>) -> Result<(), AssertionFailure>;

    /// Short, stable human-readable description shown in test output.
    fn describe(&self) -> String;
}

/// One named test case. May or may not target a specific bundled
/// architecture — `architecture` is `None` for tests that ship their
/// own inline source.
#[derive(Debug)]
pub struct TestCase {
    pub name: String,
    pub architecture: Option<String>,
    pub bindings: IndexMap<String, String>,
    pub assertions: Vec<Box<dyn Assertion>>,
}

/// Bundle of cases driven through one runner pass. Suites compose
/// into a single typed TestReport.
#[derive(Debug, Default)]
pub struct TestSuite {
    pub cases: Vec<TestCase>,
}

impl TestSuite {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn case(&mut self, c: TestCase) {
        self.cases.push(c);
    }
}

/// Per-case outcome: name + assertion-level pass/fail tally.
#[derive(Debug, Clone)]
pub struct CaseOutcome {
    pub name: String,
    pub passed: usize,
    pub failures: Vec<AssertionFailure>,
}

impl CaseOutcome {
    #[must_use]
    pub fn ok(&self) -> bool {
        self.failures.is_empty()
    }
}

/// Aggregate report across every case in a [`TestSuite`].
#[derive(Debug, Clone, Default)]
pub struct TestReport {
    pub cases: Vec<CaseOutcome>,
}

impl TestReport {
    #[must_use]
    pub fn passed(&self) -> usize {
        self.cases.iter().filter(|c| c.ok()).count()
    }
    #[must_use]
    pub fn failed(&self) -> usize {
        self.cases.iter().filter(|c| !c.ok()).count()
    }
    #[must_use]
    pub fn total_assertions(&self) -> usize {
        self.cases
            .iter()
            .map(|c| c.passed + c.failures.len())
            .sum()
    }
    #[must_use]
    pub fn ok(&self) -> bool {
        self.cases.iter().all(CaseOutcome::ok)
    }
}

/// Run one case against an already-rendered architecture. Returns the
/// per-case outcome (passed-count + collected failures).
#[must_use]
pub fn run_case_against(case: &TestCase, ctx: &AssertContext<'_>) -> CaseOutcome {
    let mut outcome = CaseOutcome {
        name: case.name.clone(),
        passed: 0,
        failures: Vec::new(),
    };
    for a in &case.assertions {
        match a.check(ctx) {
            Ok(()) => outcome.passed += 1,
            Err(mut f) => {
                f.assertion = a.describe();
                outcome.failures.push(f);
            }
        }
    }
    outcome
}

/// Typed failure record — one per failing assertion.
#[derive(Debug, Clone, Serialize, Deserialize, Error)]
#[error("{assertion}: {message}")]
pub struct AssertionFailure {
    /// `Assertion::describe()` output, filled in by the runner.
    #[serde(default)]
    pub assertion: String,
    pub message: String,
    /// Optional path into the terraform.json shape (e.g.
    /// `/resource/aws_vpc/main/cidr_block`) for richer error UX.
    #[serde(default)]
    pub pointer: Option<String>,
}

impl AssertionFailure {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            assertion: String::new(),
            message: message.into(),
            pointer: None,
        }
    }

    #[must_use]
    pub fn at(mut self, pointer: impl Into<String>) -> Self {
        self.pointer = Some(pointer.into());
        self
    }

    #[must_use]
    /// Internal: convenience constructor for the architecture-render
    /// path inside [`AssertContext::from_architecture`].
    #[must_use]
    pub fn render(e: impl Into<String>) -> Self {
        Self::new(e.into())
    }
}
