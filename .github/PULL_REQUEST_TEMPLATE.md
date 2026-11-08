<!-- Thanks! A pull request here is squash-merged, so the title becomes the commit
     subject on main: say what it does, in the imperative. -->

## What and why

<!-- What changes, and why. For a bug: how it was established, not assumed. Link the issue. -->

## Checklist

- [ ] `scripts/ci.sh` ends in `ci: all green` locally
- [ ] Tests added: one fails without this change (for a fix, I checked by reverting it
      where it is called, not only in a helper)
- [ ] `CHANGELOG.md` has an entry under *Unreleased*
- [ ] Every commit has a DCO sign-off (`git commit -s`) matching its author
- [ ] No JavaScript added (no `.js`, `.mjs`, `<script>` or inline handler)
- [ ] No dependency that compiles C or C++ (no `-sys` crate with a build script, no `cc`)
