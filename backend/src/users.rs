use std::convert::TryFrom;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum UserRole {
    Default = 0,
    Admin = 1,
}

impl TryFrom<i32> for UserRole {
    type Error = ();

    fn try_from(val: i32) -> Result<Self, Self::Error> {
        match val {
            0 => Ok(UserRole::Default),
            1 => Ok(UserRole::Admin),
            _ => Err(()),
        }
    }
}
