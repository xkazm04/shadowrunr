//! Drift gate on the Rust side: the committed `packages/contract/schema/*.json` must equal a
//! fresh export, so changing a wire type without regenerating the TS contract fails
//! `cargo test`. Regenerate with `pnpm -C packages/contract generate`.

use std::path::PathBuf;

#[test]
fn committed_schema_matches_the_rust_types() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packages/contract/schema");
    for (name, fresh) in shadowrunr_types::schema::export_files() {
        let path = dir.join(name);
        let committed = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
            .replace("\r\n", "\n");
        assert!(
            committed == fresh,
            "{} is stale: run `pnpm -C packages/contract generate`",
            path.display()
        );
    }
}
