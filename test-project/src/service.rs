use crate::user::{User, UserUpdate};

/// Service for managing users.
pub struct UserService {
    users: Vec<User>,
    next_id: u32,
}

impl UserService {
    /// Create a new user service.
    pub fn new() -> Self {
        Self {
            users: Vec::new(),
            next_id: 1,
        }
    }

    /// CREATE: Create a new user with auto-generated ID
    pub fn create_user(&mut self, name: &str, email: &str) -> &User {
        let user = User::new_with_default_role(self.next_id, name, email);
        self.users.push(user);
        self.next_id += 1;
        self.users.last().unwrap()
    }

    /// CREATE: Add a user to the service (with existing User)
    pub fn add_user(&mut self, user: User) {
        self.users.push(user);
    }

    /// READ: Find a user by email.
    pub fn find_by_email(&self, email: &str) -> Option<&User> {
        self.users.iter().find(|u| u.has_email(email))
    }

    /// READ: Find a user by id.
    pub fn find_by_id(&self, id: u32) -> Option<&User> {
        self.users.iter().find(|u| u.has_id(id))
    }

    /// READ: Find a user by id (mutable reference)
    pub fn find_by_id_mut(&mut self, id: u32) -> Option<&mut User> {
        self.users.iter_mut().find(|u| u.has_id(id))
    }

    /// READ: List all users.
    pub fn list_all(&self) -> &[User] {
        &self.users
    }

    /// READ: Count users.
    pub fn count(&self) -> usize {
        self.users.len()
    }

    /// READ: Get all emails from all users.
    pub fn get_all_emails(&self) -> Vec<String> {
        self.users.iter().map(|user| user.email().to_string()).collect()
    }

    /// UPDATE: Update a user by id with partial data
    pub fn update_user(&mut self, id: u32, update: UserUpdate) -> Result<(), String> {
        if let Some(user) = self.find_by_id_mut(id) {
            user.update(update);
            Ok(())
        } else {
            Err(format!("User with id {} not found", id))
        }
    }

    /// DELETE: Remove a user by email.
    pub fn remove_by_email(&mut self, email: &str) -> bool {
        if let Some(pos) = self.users.iter().position(|u| u.has_email(email)) {
            self.users.remove(pos);
            true
        } else {
            false
        }
    }

    /// DELETE: Remove a user by id.
    pub fn remove_by_id(&mut self, id: u32) -> bool {
        if let Some(pos) = self.users.iter().position(|u| u.has_id(id)) {
            self.users.remove(pos);
            true
        } else {
            false
        }
    }

    /// Check if service is empty.
    pub fn is_empty(&self) -> bool {
        self.users.is_empty()
    }
}
