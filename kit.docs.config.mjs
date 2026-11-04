// kit.docs.config.mjs — what kit-check-docs holds this repository's docs to
// (https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/docs.md).
// `roots` are the globs whose every file must be owned by exactly one reference doc's `Owns:`
// line; `exempt` is the short list of things no doc should own, each with its reason.
export default {
  docs: "docs",
  roots: ["crates/*", "app"],
  exempt: new Map([
    ["**/tests/**", "tests are pinned by the reference they test, not owned by it"],
    ["**/fixtures/**", "test data, owned by the tests that read it"],
    ["**/target/**", "cargo build output; gitignored"],
  ]),
};
