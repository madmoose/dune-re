//! Offline validation of the presentation shader. wgpu only checks dune.wgsl at
//! pipeline-creation time on a live GPU; this parses and validates it with naga
//! (wgpu's own frontend) so shader regressions are caught in CI without one.

#[test]
fn dune_wgsl_parses_and_validates() {
    let src = include_str!("../src/dune.wgsl");

    let module = naga::front::wgsl::parse_str(src)
        .unwrap_or_else(|e| panic!("dune.wgsl failed to parse:\n{}", e.emit_to_string(src)));

    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator
        .validate(&module)
        .unwrap_or_else(|e| panic!("dune.wgsl failed validation:\n{}", e.emit_to_string(src)));
}
