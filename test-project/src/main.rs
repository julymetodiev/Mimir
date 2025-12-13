mod user;
mod service;

use service::UserService;
use user::UserUpdate;

fn main() {
    let mut service = UserService::new();

    // CREATE: Create users
    println!("=== Creating users ===");
    let user1 = service.create_user("Alice", "alice@example.com");
    let user2 = service.create_user("Bob", "bob@example.com");
    println!("Created user: {} (ID: {})", user1.name(), user1.id());
    println!("Created user: {} (ID: {})", user2.name(), user2.id());
    println!("Total users: {}", service.count());
    println!();

    // READ: Find users
    println!("=== Reading users ===");
    if let Some(found) = service.find_by_email("alice@example.com") {
        println!("Found user by email: {} ({})", found.name(), found.email());
    }
    
    if let Some(found) = service.find_by_id(2) {
        println!("Found user by ID: {} ({})", found.name(), found.email());
    }
    
    println!("All users:");
    for user in service.list_all() {
        println!("  - ID {}: {} ({})", user.id(), user.name(), user.email());
    }
    println!();

    // UPDATE: Update a user with partial data
    println!("=== Updating user ===");
    let update_data = UserUpdate {
        name: Some("Alice Smith".to_string()),
        email: None, // Keep original email
    };
    
    match service.update_user(1, update_data) {
        Ok(_) => println!("User 1 updated successfully"),
        Err(e) => println!("Update failed: {}", e),
    }
    
    // Verify update
    if let Some(updated) = service.find_by_id(1) {
        println!("Updated user: {} ({})", updated.name(), updated.email());
    }

    // Update with new email
    let email_update = UserUpdate {
        name: None,
        email: Some("alice.smith@newdomain.com".to_string()),
    };
    
    service.update_user(1, email_update);
    if let Some(updated) = service.find_by_id(1) {
        println!("Updated email: {} ({})", updated.name(), updated.email());
    }
    println!();

    // DELETE: Remove users
    println!("=== Deleting users ===");
    let removed = service.remove_by_id(1);
    println!("Removed user ID 1: {}", removed);
    
    let removed = service.remove_by_email("bob@example.com");
    println!("Removed user with email bob@example.com: {}", removed);
    
    println!("Remaining users: {}", service.count());
    println!("Service empty: {}", service.is_empty());
}