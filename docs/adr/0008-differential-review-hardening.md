# ADR 0008: Second-Pass Hardening from the Differential Review (F1–F12)

## Status

Accepted

## Date

2026-06-26

## Context

After the 0.5.0 hardening defaults landed (ADR 0007) and the resource limits of
ADR 0004 were in place, a focused **differential security review** of the
`feat/harden` branch was run against `main`. It covered the changed surface plus
the new XPath 2.0 subsystem, and reproduced **twelve** issues with compiled
proof-of-concept programs. They cluster into three root causes:

1. **The new XPath 2.0 engine shipped with no runtime safety.** Unlike the
   XPath 1.0 evaluator (which has a node-visit budget) the 2.0 evaluator had no
   work budget and no evaluator-side recursion guard, and its integer arithmetic
   was unchecked. A tiny expression could exhaust CPU/memory, overflow the
   stack, or panic.

2. **Two existing limits were charged in the wrong place, so they were
   bypassable.** The XSD-regex match-step budget counted one tick per
   `match_node` entry while a single entry could do O(N) allocation work; the
   XPath 1.0 node-visit budget charged node-set *construction* but not the
   O(n·m) node-set *comparison*. Both let an attacker do super-linear work while
   staying under the configured cap.

3. **A handful of residual panics / injection / output-correctness gaps**
   survived the first hardening pass: an unsanitized processing-instruction
   target, fixed byte-index slicing in three datetime validators, a `substring`
   integer overflow, a duplicate-namespace-declaration serialization bug, and a
   deep-linear-entity-chain stack overflow not covered by the byte budget.

A recurring lesson: **a resource limit only works if it is charged where the
cost is actually incurred, and a recursion guard only works if it covers every
way the recursion can be built** — including iteratively-parsed flat operator
chains whose depth the parser's nesting cap never saw.

## Decision

Fix all twelve findings, preferring corrections that *preserve behaviour for
legitimate input* (lazy allocation, charging the real cost) over blanket
rejection, and add a dedicated regression suite (`tests/hardening_regressions.rs`)
that pins each one.

### New / changed limits

Following ADR 0004's "default constant + per-type builder" pattern:

| Constant | Value | Applies to | Builder |
|----------|-------|------------|---------|
| `xpath2::DEFAULT_MAX_XPATH2_WORK` | 10 000 000 | Total evaluation work (nodes selected, comparisons, sequence items) per XPath 2.0 evaluation | `XPath2Evaluator::with_max_work(usize)` |
| `xpath2::DEFAULT_MAX_XPATH2_EVAL_DEPTH` | 1 000 | XPath 2.0 evaluator AST recursion depth | `XPath2Evaluator::with_max_eval_depth(usize)` |
| XPath 2.0 parser node cap | `max_depth × 64` (≥ 1024) | Total AST nodes built by the XPath 2.0 parser | (derived from `with_max_depth`) |
| `parser::DEFAULT_MAX_ENTITY_DEPTH` | 256 | Entity replacement-text expansion nesting depth | (internal; bounded alongside the byte budget) |

### Per-finding decisions

- **F1 — XPath 2.0 stack overflow via flat operator chains.** Left-associative
  chains (`1 or 1 or …`) are parsed iteratively, so they escaped the parser's
  nesting cap and built an arbitrarily deep AST that overflowed the stack on
  evaluation *and on drop* (recursive `Box<Expr>` destructor). The parser now
  charges every binary/unary node against a total **node-count cap**, which
  bounds the whole tree — fixing build, eval, and drop. A runtime evaluator
  depth guard (`EvalBudget::enter`, RAII-released) backs this up.

- **F2 — XPath 2.0 has no evaluation budget.** Added a shared `EvalBudget`
  (work counter + depth counter, `Rc`-shared so it survives context forks),
  charged in path steps, `for` binding, sequence/union construction,
  `general_compare`, and `to` ranges. The per-`to` `max_sequence_items` cap
  remains but no longer has to carry aggregate growth.

- **F3 / F9 — XPath 2.0 integer arithmetic.** `mod` now rejects a zero divisor
  (matching `div`/`idiv`); `+`, `-`, `*`, and unary `-` use `checked_*` and
  return an error on overflow instead of panicking (debug) or wrapping
  (release).

- **F4 — XSD regex quadratic allocation.** `match_repetition` allocated an
  O(N) `seen` bitmap on entry, before knowing whether the repetition could
  advance. An outer repetition calling it O(N) times made that O(N²) — work the
  per-entry step budget never charged. The bitmap is now allocated **lazily**,
  only on the first productive greedy iteration, so the common
  `a*b*`-over-`aaaa…` shape stays linear and still matches correctly.

- **F5 — XPath 1.0 node-set comparison uncharged.** `=`/`!=` over node-sets is
  an O(n·m) string-value scan. It is now charged against the existing node-visit
  budget (`charge_comparison`, proportional to the operand cardinalities), so a
  comparison built from cheap child-axis operands can no longer run for minutes
  under the cap.

- **F6 — Processing-instruction target injection.** A PI target containing `?>`
  plus markup broke out of PI position. `sanitize_pi_target` now validates the
  target as an XML NCName (collapsing invalid targets to `_`) in addition to
  renaming the reserved `xml` target. Both the `XmlWriter` and DOM serializer
  share the helper.

- **F7 — Deep linear entity chain.** A non-cyclic chain `e0 → e1 → … → eN` with
  a tiny leaf expands to ~1 byte (so the byte budget never trips) yet recurses
  N frames deep. Expansion depth (`seen.len()`) is now capped at
  `DEFAULT_MAX_ENTITY_DEPTH`, failing closed with a normal error.

- **F8 — datetime multibyte panic.** `is_valid_gmonth` / `is_valid_gday` /
  `is_valid_gmonthday` sliced fixed byte ranges after only length checks,
  panicking when a multibyte character straddled a boundary. They now reject
  non-ASCII input up front (these lexical forms are entirely ASCII).

- **F10 — duplicate namespace declarations.** Two distinct invalid prefixes both
  sanitize to `_`, which emitted duplicate `xmlns:_` attributes (not
  well-formed). The serializer now disambiguates colliding prefixes (preserving
  both URIs) and skips an impossible duplicate default-namespace declaration.

- **F11 — `substring()` overflow.** `start + len` overflowed `usize` for
  `inf`/huge length arguments. It now uses `saturating_add` and clamps, so the
  result is the (clamped) substring with no panic in either build profile.

- **F12 — unknown Unicode property/block names.** The 0.5.0 change to reject
  unknown property names at compile time (so `\P{IsTypo}` cannot match every
  character) was reviewed for over-restriction. On audit the block table already
  matches the XSD 1.0 Part 2 Appendix-F **closed** list, so the fail-closed
  rejection is spec-correct, not a regression. This was resolved by
  **documentation only**: `is_known_property_name` and `match_unicode_block` now
  state that the recognized set is the XSD-defined closed list, so a future
  reader does not "loosen" it back into the bypass. No behavioural change.

## Consequences

- The XPath 2.0 engine now fails closed on adversarial expressions the same way
  the 1.0 engine does. The new defaults are sized far above realistic
  expressions (the full XPath 2.0 conformance suite is unaffected) and are
  configurable via builders for callers with unusual needs.
- F1's node-count cap rejects pathologically large *flat* expressions
  (tens of thousands of chained operators). This is a behaviour change only for
  machine-generated expressions of that size; raise `with_max_depth` to lift it.
- F4 and F5 change pathological cases from "slow success" to "fast, clean
  error", and F3/F8/F9/F11 change pathological cases from "panic" to "error".
  Legitimate input is unaffected — each fix ships with a "valid input still
  works" regression assertion.
- All W3C conformance suites remain at their prior pass rates (XML 100%, XSTS
  NIST 100%, MS 100%, Sun 100%), the library still builds with zero warnings,
  and `tests/hardening_regressions.rs` guards every finding against recurrence.
