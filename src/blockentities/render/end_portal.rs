//! End portals and gateways. Their starfield needs its own shader, which the
//! native renderer doesn't have yet, so nothing draws them.

#[cfg(test)]
pub(super) fn test_states(out: &mut Vec<crate::blockentities::BeState>) {
    out.push(super::test_state("end_portal", &[]));
    out.push(super::test_state("end_gateway", &[]));
}
