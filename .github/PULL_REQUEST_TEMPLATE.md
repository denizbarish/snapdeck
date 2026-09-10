## What this changes

<!-- One or two sentences. Why it looks the way it does belongs in the commit body. -->

## Related issue

<!-- Fixes #123, or "none" for a small fix. -->

## Gates

All six pass locally. See [CONTRIBUTING.md](../blob/main/CONTRIBUTING.md#the-quality-gates).

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `pnpm lint`
- [ ] `pnpm test`
- [ ] `pnpm build`

## Also

- [ ] New behaviour has a test, or the body says why it cannot.
- [ ] Docs that this change makes wrong are updated.
- [ ] Nothing here promises a feature that does not exist.
