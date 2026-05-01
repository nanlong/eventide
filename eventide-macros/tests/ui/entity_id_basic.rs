use eventide_macros::entity_id;
use uuid::Uuid;

#[entity_id]
struct UserId(Uuid);

#[entity_id(debug = false)]
struct ProfileId(Uuid);

impl std::fmt::Debug for ProfileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ProfileId(..)")
    }
}

fn main() {
    let id = UserId::new(Uuid::new_v4());
    let _ = format!("{:?}", id); // Debug is auto-derived by default, so formatting must work.

    let pid = ProfileId::new(Uuid::new_v4());
    let _ = format!("{:?}", pid); // Uses the hand-written Debug impl above; the fact that this
                                  // compiles proves the macro did not auto-derive Debug (which
                                  // would have caused a duplicate-impl error).
}
