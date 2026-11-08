<!-- Thanks! This pull request is squash-merged into `main`, so its title becomes the commit
     subject: say what it does, in the imperative. Dependabot's pull requests need none of
     this; they merge once the checks pass. -->

## What and why

<!-- What changes, and why. For a bug: how it was established, not assumed.
     Link the issue: "Fixes #123". A feature needs an agreed issue first. -->

## Checklist

- [ ] `scripts/ci.sh` ends in `ci: all green` locally
- [ ] Tests added: one fails without this change (for a fix, I checked by reverting it
      where it is called, not only in a helper)
- [ ] `CHANGELOG.md` has an entry under *Unreleased*
- [ ] Every commit has a DCO sign-off (`git commit -s`) matching its author
- [ ] No JavaScript added (no `.js`, `.mjs`, `<script>` or inline handler)
- [ ] No dependency that compiles C or C++ (no `-sys` crate with a build script, no `cc`)
- [ ] If an agent wrote any of this: I read the diff and I can answer for every line
