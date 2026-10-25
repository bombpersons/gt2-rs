# Licence awards and saved achievements

Native award rules now reproduce strict medal comparisons, the qualifying
attempt counter, retained better grades and group completion. They run
separately from driving/rendering. The result/replay front end must still
produce their commit event. Name entry, best-record insertion, result/replay
presentation, return/retry flow and native saves remain unfinished.

## Original boundaries

`800156EC` latches the race outcome and time. The Finish/prize sequence and
automatic replay precede the menu's `8004DD80` award update. A fresh 0.4.2 B-1
run finished at 38.675 s and displayed its silver prize, entered replay, then
updated the saved grade only after Start → Exit closed replay. The best-time
slot stayed empty until name-entry confirmation subsequently invoked
`8005DEFC`. That insertion is separate from the award rule.

`8004E184..1A8` skips award updates for unsuccessful attempts. For a success,
`8004DE14..E6C` compares unsigned time strictly below four ordered GTDT limits.
The saved ranks are gold 4, silver 3, bronze 2, assisted pass 1 and unpassed 0.
Equality does not earn that band, though it can earn the next slower one.

The fourth band has an additional qualification. If the saved grade is zero,
`8004DE8C..EA0` increments the record's `+2` byte, saturating at 255.
`8004DEA4..EC8` compares the incremented count with GTDT row `+0x74`. Before
that count is reached, no grade is earned. Already passed tests do not
increment it. This counts qualifying fourth-band results, not all attempts.
`8004DECC..EE0` only raises the saved grade; worse later attempts do not
downgrade an achievement.

`8004DCAC` checks all ten saved group grades for nonzero; `8004DD14` checks
them for grade four. Their caller checks before and after the award to
detect newly passed/all-gold groups.

## Native contract

`LicenceGrade`, `LicenceAchievement` and `LicenceProgress` are public native
state. The progress map uses content licence ids and round-trips through RON.
Group checks accept explicit membership, allowing authored group sizes.
`LicenceDefinition` retains its four limits and adds optional
`assisted_pass_attempts`; None disables assisted passing for authored/older
definitions. Re-export for original qualification data.

`LicenceAwardsPlugin` requires no renderer, vehicle or session. A front end
emits one `CommitLicenceAward` after returning from the attempt/replay and
receives `LicenceAwardApplied`, including whether its grade improved. Failure
uses no time and leaves progress unchanged. The plugin is separately
selectable in `GamePlugins`. It does not attach a premature commit to
`LicenceRun` or update the in-race records panel.

The exporter reads each of the sixty qualification bytes and writes the
selected definition. It imports saved achievement slots in the six-by-ten
source order into `race/licence-progress.initial.ron`, omitting default zero
entries. The active key/order is checked and invalid grades rejected. Output
assets, emulator data and numerical fixtures remain private and ignored.

## Executed checks

`tools/record_licence_awards.py` resumes the actual menu immediately before
its update. Input pokes change the time, saved grade/counter, selected key
and group neighbors. Original execution, write watches and traces supply
expected achievements and returned group predicates; no native formula
supplies the expected values.

- Fifty initial cases cover every time limit at -1/equality/+1 ms, existing
  grades, exact qualification-count boundaries, saturation, failure skips,
  retention and newly all-gold groups.
- Sixty fresh cases recorded after implementation select every actual GTDT
  key and corresponding saved group/test slot, varying band, equality,
  existing grade, required count and neighboring grades.
- All 110 cases and 424 original group-predicate executions match exactly.
- A natural completed silver result was exported through native RON. All
  sixty imported achievements and the selected qualification setting match.
- An independently authored rules-only test checks deferred commit, repeated
  slow attempts, improvement notifications, failure and slower-result retention.

These establish the achievement consumer and its data boundary. The native
result/replay/return controller must still invoke it in normal play.

Validation also passes the full workspace suite and the four records-panel
checks. A complete isolated asset export generated 160 manifest entries,
including the new progress file and selected qualification setting. Formatting
and source-content audits pass. Clippy completes with the existing nested-if
suggestion in `game/tests/wheel_rotation.rs`.

```sh
python3 tools/record_licence_awards.py
python3 tools/record_licence_awards.py --fresh
python3 tools/record_licence_awards.py --export-progress .psx/awards/menu-commit/ram.bin
cargo test -p gran_turismo_2 --all-features --test licence_awards --test records
```

Checkpoint recipe: from the current exporter's `start/state.bin`, run 2,500
fields with Cross at 0..2210, then Square. Continue 300 fields with Cross at
80..92, then 500 with Cross at 200..212, entering automatic replay. Run 500
with Start at 100..112. From that paused state run 235 with Down at 10..22 and
Cross at 80..92. The award executes at relative field 1 of the next four-field
probe. Commands, builds and state hashes are retained in `.psx/awards/` and
the ignored reference directory.
