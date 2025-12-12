use crate::user::User;

/// Service for managing users.
pub struct UserService {
    users: Vec<User>,
}

impl UserService {
    /// Create a new user service.
    pub fn new() -> Self {
        Self { users: Vec::new() }
    }

    /// Add a user to the service.
    pub fn create_user(&mut self, user: User) {
        self.users.push(user);
    }

    /// Find a user by email.
    pub fn find_by_email(&self, email: &str) -> Option<&User> {
        self.users.iter().find(|u| u.has_email(email))
    }

    /// List all users.
    pub fn list_all(&self) -> &[User] {
        &self.users
    }

    /// Count users.
    pub fn count(&self) -> usize {
        self.users.len()
    }

    /// Remove a user by email.
    pub fn remove_by_email(&mut self, email: &str) -> bool {
        if let Some(pos) = self.users.iter().position(|u| u.has_email(email)) {
            self.users.remove(pos);
            true
        } else {
            false
        }
    }

    /// Clear all users.
    pub fn clear_all(&mut self) {
        self.users.clear();
    }

    /// Check if service is empty.
    pub fn is_empty(&self) -> bool {
        self.users.is_empty()
    }
}
