# lava-test

Typed TDD/BDD framework for lava architectures and the wider
tatara-lisp ecosystem.

## Shape

```text
TestSuite { cases: Vec<TestCase> }
  TestCase { name, architecture?, bindings, assertions: Vec<Box<dyn Assertion>> }
    Assertion (trait): check(&AssertContext) -> Result<(), AssertionFailure>
      AssertContext { architecture, terraform_json }

run_case_against(&case, &ctx) → CaseOutcome
TestReport.ok() === every CaseOutcome.ok()
```

## Built-in lava assertion variants

| Variant            | Checks                                                |
|---|---|
| `ResourceExists`   | `<type_id>.<name>` is in the rendered shape           |
| `NoResource`       | `<type_id>.<name>` is *not* in the rendered shape     |
| `AttributeEquals`  | `<type>.<name>.<attr>` equals JSON value              |
| `ResourceCount`    | Total resources of `<type_id>` equals N               |
| `OutputEquals`     | `output.<name>.value` equals JSON value               |
| `RefValid`         | Every `${type.name.attr}` ref resolves                |

## Composing with custom assertions

`Assertion` is a trait — other tatara-lisp consumers implement it for
their domain:

```rust
use lava_test::{Assertion, AssertContext, AssertionFailure};

#[derive(Debug)]
struct EveryCaixaShipsFlakeNix;

impl Assertion for EveryCaixaShipsFlakeNix {
    fn check(&self, _ctx: &AssertContext<'_>) -> Result<(), AssertionFailure> {
        // ... walk the caixa source ...
        Ok(())
    }
    fn describe(&self) -> String {
        "every-caixa-ships-flake-nix".into()
    }
}
```

The runner dispatches on the trait object — no built-in variant is
privileged. Domain-specific test suites compose with the lava
assertions in the same `TestCase`.

## Tests

`cargo test --release` runs 11 unit tests covering every built-in
assertion variant + the runner aggregation.
