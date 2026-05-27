use crate::{
    AuthMethod, IdpIdentity, Principal, RunProvenance, RunServerProvenance, SystemActorKind,
};

#[must_use]
pub fn test_principal() -> Principal {
    Principal::user(
        IdpIdentity::new("fabro:test", "test-user").expect("test identity should be valid"),
        "test".to_string(),
        AuthMethod::DevToken,
    )
}

#[must_use]
pub fn test_run_provenance() -> RunProvenance {
    RunProvenance {
        server:  Some(RunServerProvenance {
            version: "test".to_string(),
        }),
        client:  None,
        subject: test_principal(),
    }
}

/// Provenance attributed to the engine itself, with no server/client metadata.
/// Used in tests that exercise system-initiated runs and serde round-trips.
#[must_use]
pub fn engine_run_provenance() -> RunProvenance {
    RunProvenance {
        server:  None,
        client:  None,
        subject: Principal::System {
            system_kind: SystemActorKind::Engine,
        },
    }
}
