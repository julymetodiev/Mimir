/// A user in the system.
#[derive(Debug, Clone)]
pub struct User {
    id: u32,
    name: String,
    email: String,
}
    /// Create a new user.
    pub fn new(id: u32, name: &str, email: &str) -> Self {
        Self {
            id,
            name: name.to_string(),
            email: email.to_string(),
        }
    }

    /// Get the user's id.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Get the user's name.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Check if email matches.
    pub fn has_email(&self, email: &str) -> bool {
        self.email == email
    }

    /// Check if id matches.
    pub fn has_id(&self, id: u32) -> bool {
        self.id == id
    }

    /// Check if email matches.
    pub fn has_email(&self, email: &str) -> bool {
        self.email == email
    }

    /// Update the user email.
    pub fn set_email(&mut self, email: &str) {
        self.email = email.to_string();
    }