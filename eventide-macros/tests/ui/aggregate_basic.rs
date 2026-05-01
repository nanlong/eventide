use eventide_macros::entity;
use serde::{Deserialize, Serialize};

#[entity(id = String)]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Account {
    name: String,
}

fn main() {}
