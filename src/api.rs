use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

impl Role {
    /// The wire string for this role (both providers use the same spellings).
    pub fn as_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

/// refac's provider-agnostic chat message. A turn carries one or more text
/// `fields` (a transform turn is `[selected, transform]`); each backend adapts
/// this to its own wire format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub fields: Vec<String>,
}

impl Message {
    pub fn system<S: Into<String>>(content: S) -> Message {
        Message {
            role: Role::System,
            fields: vec![content.into()],
        }
    }

    pub fn user(fields: Vec<String>) -> Message {
        Message {
            role: Role::User,
            fields,
        }
    }
}
