/// User role enum
#[derive(Debug, Clone, PartialEq)]
pub enum UserRole {
    Admin,
    Moderator,
    User,
}

/// A user in the system.
/// A user in the system.
#[derive(Debug, Clone)]
pub struct User {
    id: u32,
    name: String,
    email: String,
    role: UserRole,
    is_active: bool,
/// Struct for partial updates to a user
#[derive(Debug, Default)]
pub struct UserUpdate {
    pub name: Option<String>,
    pub email: Option<String>,
    pub role: Option<UserRole>,
    pub is_active: Option<bool>,
}
    pub role: Option<UserRole>,
}
    /// Create a new user.
    pub fn new(id: u32, name: &str, email: &str, role: UserRole) -> Self {
        Self {
            id,
            name: name.to_string(),
            email: email.to_string(),
            role,
            is_active: true,
        }
    }
            role,
        }
    }

    /// Create a new user with default User role.
    pub fn new_with_default_role(id: u32, name: &str, email: &str) -> Self {
        Self::new(id, name, email, UserRole::User)
    }

    /// Get the user's id.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Get the user's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the user's email.
    pub fn email(&self) -> &str {
        &self.email
    }

    /// Set the user's role.
    pub fn set_role(&mut self, role: UserRole) {
        self.role = role;
    }

    /// Get the user's active status.
    pub fn is_active(&self) -> bool {
        self.is_active
    }

    /// Set the user's active status.
    pub fn set_active(&mut self, is_active: bool) {
        self.is_active = is_active;
    }

    /// Deactivate the user.
    pub fn deactivate(&mut self) {
    /// Update the user with partial data.
    pub fn update(&mut self, update: UserUpdate) {
        if let Some(name) = update.name {
            self.name = name;
        }
        if let Some(email) = update.email {
            self.email = email;
        }
        if let Some(role) = update.role {
            self.role = role;
        }
        if let Some(is_active) = update.is_active {
            self.is_active = is_active;
        }
    }
        if let Some(role) = update.role {
            self.role = role;
        }
    }
}