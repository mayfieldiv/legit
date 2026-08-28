//! Unit tests for `CanonicalPathBuf`. These touch the real filesystem via
//! tempdirs — canonicalization is exactly the I/O the type exists to prove.

use super::CanonicalPathBuf;

#[test]
fn dot_and_dotdot_segments_normalize_to_the_same_identity() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("sub");
    std::fs::create_dir(&sub).unwrap();

    let direct = CanonicalPathBuf::canonicalize(&sub).unwrap();
    let dotted = CanonicalPathBuf::canonicalize(dir.path().join("./sub/../sub")).unwrap();
    assert_eq!(direct, dotted);
}

#[cfg(unix)]
#[test]
fn a_symlinked_route_resolves_to_the_target_identity() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    std::fs::create_dir(&target).unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    assert_eq!(
        CanonicalPathBuf::canonicalize(&target).unwrap(),
        CanonicalPathBuf::canonicalize(&link).unwrap(),
    );
}

#[test]
fn a_missing_path_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(CanonicalPathBuf::canonicalize(dir.path().join("nope")).is_err());
}
