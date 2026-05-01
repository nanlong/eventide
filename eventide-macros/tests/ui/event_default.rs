use eventide_macros::domain_event;
use serde::{Deserialize, Serialize};

#[domain_event(version = 1)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum UserEvent {
    Created {
        name: String,
    },
}

fn main() {}
