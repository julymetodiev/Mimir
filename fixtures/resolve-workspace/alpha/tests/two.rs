//! A test binary is a crate of its own, so this `shared` has the same symbol as the other's.

fn shared() -> u32 {
    1
}

#[test]
fn two_calls() {
    assert_eq!(shared(), 1);
}
