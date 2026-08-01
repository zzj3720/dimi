//! Id newtypes mirroring the min-1 string id schemas (`schema.ts` lines
//! 11–15). Beyond `min(1)` zod does not constrain id contents; neither do we.

use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;
use std::str::FromStr;

/// Error returned when an id string is empty (the only rule `min(1)` adds).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmptyIdError;

impl fmt::Display for EmptyIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("id must not be empty")
    }
}

impl std::error::Error for EmptyIdError {}

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// The id as a string slice.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> String {
                value.0
            }
        }

        impl FromStr for $name {
            type Err = EmptyIdError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                if s.is_empty() {
                    return Err(EmptyIdError);
                }
                Ok($name(s.to_owned()))
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let s = String::deserialize(deserializer)?;
                if s.is_empty() {
                    return Err(serde::de::Error::custom(concat!(
                        stringify!($name),
                        " must not be empty"
                    )));
                }
                Ok($name(s))
            }
        }
    };
}

id_type!(TurnId); // `turnIdSchema` — `z.string().min(1)`.
id_type!(StepId); // `stepIdSchema` — `z.string().min(1)`.
id_type!(FrameId); // `frameIdSchema` — `z.string().min(1)`.
id_type!(TaskId); // `taskIdSchema` — `z.string().min(1)`.
id_type!(AgentId); // `agentIdSchema` — `z.string().min(1)`.

/// `isPlainAgentId` (`schema.ts` lines 24–34): the filename-safe agent id
/// shape `^[A-Za-z0-9._-]{1,128}$`, excluding `.` and `..`.
pub fn is_plain_agent_id(id: &str) -> bool {
    (1..=128).contains(&id.len())
        && id != "."
        && id != ".."
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}
