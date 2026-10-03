//! Guards Cargo.lock resolution invariants that cargo cannot express.
//!
//! `gpu-allocator 0.28` accepts `windows >=0.58,<=0.62`, a range that
//! spans semver-incompatible majors, so a full re-resolution is free
//! to unify it onto any other `windows` major the lock happens to
//! carry (0.61 came in through notify-rust's Windows backend until
//! that moved to 0.62). wgpu-hal 29 compiles its DX12 suballocator
//! against `windows 0.62` types, so that unification breaks the
//! Windows build (ID3D12Device / D3D12_RESOURCE_DESC mismatches),
//! and only on Windows, which local Linux gates never see. It has
//! happened twice (fixed in 155a572, regressed by a lock refresh on
//! 2026-07-10); this test makes the third time a red test run
//! instead of a broken nightly.
//!
//! Reading the resolved version is two cases, and missing the second
//! one silently broke this guard once already: Cargo puts a version on
//! a dependency line ONLY when the package name is ambiguous. While the
//! lock carried several `windows` majors the edge read
//! `windows 0.62.2`; once the family was unified to one version the
//! same edge became a bare `windows`, the `starts_with("windows ")`
//! lookup found nothing, and the test failed claiming gpu-allocator had
//! no `windows` dependency at all. Both spellings mean the same thing
//! and both have to resolve.

use std::path::Path;

/// Returns the dependency lines of `package`'s block in Cargo.lock.
fn lock_dependencies(lock: &str, package: &str) -> Vec<String> {
    let header = format!("name = \"{package}\"");
    let mut in_block = false;
    let mut deps = Vec::new();
    for line in lock.lines() {
        if line.starts_with("name = ") {
            in_block = line.trim() == header;
            continue;
        }
        if in_block {
            if line.starts_with("[[package]]") {
                break;
            }
            let line = line.trim();
            if let Some(dep) = line.strip_prefix('"').and_then(|l| l.strip_suffix("\",")) {
                deps.push(dep.to_owned());
            }
        }
    }
    deps
}

#[test]
fn gpu_allocator_binds_windows_062() {
    let lock_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    let lock = std::fs::read_to_string(&lock_path).expect("read workspace Cargo.lock");

    let deps = lock_dependencies(&lock, "gpu-allocator");
    assert!(
        !deps.is_empty(),
        "gpu-allocator not found in Cargo.lock; if it left the graph, delete this test"
    );

    let windows_dep = deps
        .iter()
        .find(|d| d == &"windows" || d.starts_with("windows "))
        .expect("gpu-allocator should depend on the `windows` crate");

    // Cargo writes the version into a dependency line ONLY when the name
    // is ambiguous. Since the family was unified to a single version the
    // edge reads as a bare `windows`, so the version has to be read from
    // the package blocks instead: with exactly one of them, whatever it
    // says is what every dependant resolved onto.
    let resolved = match windows_dep.strip_prefix("windows ") {
        Some(version) => version.to_owned(),
        None => {
            let versions = package_versions(&lock, "windows");
            assert_eq!(
                versions.len(),
                1,
                "gpu-allocator's `windows` edge carries no version, which \
                 only happens when the name is unambiguous, yet Cargo.lock \
                 holds {} of them: {versions:?}",
                versions.len()
            );
            versions.into_iter().next().expect("length checked")
        }
    };
    assert!(
        resolved.starts_with("0.62"),
        "gpu-allocator resolved onto windows {resolved} instead of 0.62; \
         this breaks the Windows DX12 build against wgpu-hal 29. Find the \
         package that brought the other major into Cargo.lock and re-pin \
         the edge before pushing."
    );
}

/// Every version of `package` that has a block in Cargo.lock.
fn package_versions(lock: &str, package: &str) -> Vec<String> {
    let header = format!("name = \"{package}\"");
    let mut versions = Vec::new();
    let mut in_block = false;
    for line in lock.lines() {
        let line = line.trim();
        if line.starts_with("name = ") {
            in_block = line == header;
            continue;
        }
        if in_block && let Some(rest) = line.strip_prefix("version = \"") {
            if let Some(version) = rest.strip_suffix('"') {
                versions.push(version.to_owned());
            }
            in_block = false;
        }
    }
    versions
}
