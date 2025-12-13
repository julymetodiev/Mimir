/// A user in the system.
#[derive(Debug, Clone)]
pub struct User {
    id: u32,
    name: String,
    email: String,
}

/// Struct for partial updates to a user
#[derive(Debug, Default)]
pub struct UserUpdate {
    pub name: Option<String>,
    pub email: Option<String>,
}

impl User {
    /// Create a new user.
    pub fn new(id: u32, name: &str, email: &str) -> Self {
        Self {
            id,
            name: name.to_string(),
            email: email.to_string(),
        }
    }

    /// Create a new user with default User role.
    pub fn new_with_default_role(id: u32, name: &str, email: &str) -> Self {
        Self::new(id, name, email)
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

    /// Check if email matches.
    pub fn has_email(&self, email: &str) -> bool {
        self.email == email
    }

    /// Check if id matches.
    pub fn has_id(&self, id: u32) -> bool {
        self.id == id
    }

    /// Update the user with partial data.
    pub fn update(&mut self, update: UserUpdate) {
        if let Some(name) = update.name {
            self.name = name;
        }
        if let Some(email) = update.email {
            self.email = email;
        }
    }
}
