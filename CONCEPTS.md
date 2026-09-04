# Concepts

The words that mean something specific in this codebase. Cite these from
`docs/solutions/` and CLAUDE.md rather than redefining them.

## Ability animation

### Caster-side gesture
The half of an ability's animation played on the unit that *used* it — a weapon
stroke, a body turn, a burst thrown from the hands.
*Avoid:* actor-side gesture

An ability may have a caster-side gesture, a receiver-side treatment, both, or
neither, and the two halves are dispatched independently: an ability that shares
its receiver-side treatment with another still needs its own gesture, and
collapsing the two decisions into one is how an ability silently inherits the
wrong stroke.

### Receiver-side treatment
The half of an ability's animation played on the unit it *landed on* — the
crystals under a rooted target, the sparkle whirl over a stunned one, a swapped
body.

A receiver-side treatment is usually keyed on a status the ability applies
rather than on the ability itself, so several abilities that apply the same
status share one treatment. That sharing is deliberate: the status is what the
viewer needs to read, and duplicating it per ability would make identical states
look different.

### Impact
The receiver-side half played at the moment an ability *lands* — a projectile
arriving, a direct-effect cast resolving — as distinct from a treatment keyed on
a status that persists afterwards. The third generic animation hook, beside the
casting orb and the caster-side gesture; both of those are caster-side, and an
ability with no impact reaches its victim in silence however well it is cast.

An impact is anchored to a point on the victim (chest for the arrows, head for
Mind Blast, per the Classic client's attachment ids) and follows it. The shared
tier (`rendering/effects/school_impact.rs`) colours one burst by
`SpellSchool`; signatures keep bespoke impacts above it.

### Signature ability animation
A hand-authored, per-ability visual treatment, as opposed to the generic
fallbacks every ability gets for free (a cast bar, a projectile, a damage
number). Signature work is reserved for abilities whose identity a player is
expected to recognize on sight.

### Swing profile
The timing shape of one weapon stroke — how long it winds up, sweeps, holds at
extension, and eases back — held separately from *which* stroke is being played.
Sharing one profile shape across strokes is what lets a signature ability reuse
the ordinary attack machinery at different speeds without a second state
machine.

### Animation sandbox
A dedicated game state that plays any effect on demand against a static
combatant, so a visual can be reviewed without driving a live match to the
moment that produces it. It is where visual defects are actually caught; probes
narrow what reaches it, they do not replace it.

## Verification

### Visual probe
A headless test that asserts a visual effect's rendered result — where it ended
up in the world, which way it points, how far it spans, whether it cleans up on
every exit path.

A probe that asserts a value the code *stored* (a length field, a scale scalar,
a set of distinct roll values) rather than the geometry that was *rendered* will
pass while the effect is visibly wrong, because it restates the implementation
instead of the requirement. Probes are proven fail-first against the broken
version for the same reason.

### Byte-identity
The standing constraint that a change to the graphical layer must leave a
headless match's outcome bit-for-bit unchanged at the same seed. It is what
makes visual work safe to ship without a balance sweep, and it is checked by
re-running fixed seeds, not by inspection.

## Flagged ambiguities

- "actor-side" and "caster-side" were both used for the gesture played on the
  unit that used an ability — these are one concept, and **caster-side** is the
  settled term.
