# Calculator: reconstruction record

The recreation's engine is [`examples/sums/rust`](../../../../examples/sums/rust) (crate `sums-core`). It owns every rule and all state; the SwiftUI app on top (R2b, not built yet) only shows what it says. Its name and look are its own: only the behaviour is copied.

## How it is checked

`cargo test` in `examples/sums/rust` presses the keys of every sequence in [evidence/sequences.toml](evidence/sequences.toml) and compares the display and the expression line with what the reference said ([evidence/reference-results.toml](evidence/reference-results.toml)). **39 of 39 match.**

That number flatters us. The engine was written while looking at those 39 answers, so matching them shows only that it was fitted to them. The honest test is a second set of questions that the engine answers first: [evidence/sequences-2.toml](evidence/sequences-2.toml) holds 16 sequences aimed at the guesses below, and [evidence/predictions-2.toml](evidence/predictions-2.toml) holds the engine's answers, committed before the reference has been asked. Recording them on the phone (same script, `--pass` with the second file) and scoring the predictions is part of R2b.

## Decisions

- **Decimal arithmetic** (`rust_decimal`), not `f64`: 0.1 + 0.2 must be 0.3 (finding on 0.1 + 0.2). Inferred from one sum; the held-out `point-one-times-three` asks again.
- **Results**: up to 11 decimal places, rounded, trailing zeros dropped, thousands separators; scientific notation with 10 significant digits for very large or very small results (9.99999998e17, 1.000000002e-18).
- **Typed numbers are kept as typed** (all 18 digits, "3.0", "(-5)", "50%"); only results are formatted.
- **Precedence**: × and ÷ before + and −. Percent is relative to the left side after + and − (200 + 10% = 220) and a hundredth after × and ÷ (50 × 10% = 5).
- **Repeat equals** repeats the last operator and right-hand number, and the expression line shows it ("8+3").
- **Division by zero** shows the reference's words: "Undefined" for x ÷ 0, "Indeterminate" for 0 ÷ 0.
- **Delete** removes the last digit, the %, or a pending operator (bringing back the number before it: 7 + ⌫ 5 = 75); after a result it clears to 0.
- **Operators**: a second replaces the first; an operator first starts from 0.

## Guesses (where the evidence ran out)

Each is marked `GUESS:` in [`src/lib.rs`](../../../../examples/sums/rust/src/lib.rs). The held-out sequence that tests it is in brackets.

| Guess | Engine says | Tested by |
|---|---|---|
| After an operator the display keeps the number before it until the next digit | 5 + shows 5 | `display-after-operator`, `running-total` |
| "2 + =" drops the dangling operator | 2 | `equals-after-operator` |
| % on a result treats the result as typed | 5% | `percent-on-result` |
| ± on a result negates it | (-5) | `negate-result` |
| A digit after % is ignored; "50 % =" gives 0.5 | 5%; 0.5 | `digit-after-percent`, `percent-equals` |
| Repeat equals after a precedence sum repeats only the last step | 56, "14×4" | `repeat-after-precedence` |
| The carried value is rounded to 10 significant digits, not just shown so | 0.3 | `point-one-times-three`, `one-millionth` |
| Plain notation stops at 1e16 and below what 11 decimal places can show | 9,999,999,800,000,001; 1e-12 | `twelve-digit-result`, `sixteen-digit-result`, `one-millionth`, `one-trillionth` |

Every wrong prediction is a finding: either the guess changes (and the reason goes in the findings register), or the difference is something ixOS can't do (a record in `gaps.toml`).

## Gaps so far

None. The engine is plain Rust with one dependency; nothing yet needed ixOS beyond `cargo`. The platform questions start with the app (R2b): the FFI boundary, ForgeUI's keys, identifiers, and hosting in Device Lab.
