mod user;
mod service;

use service::UserService;
use user::UserUpdate;

fn main() {
    let mut service = UserService::new();

    // CREATE: Create users
    println!("=== Creating users ===");
    let user1_id = {
        let user = service.create_user("Alice", "alice@example.com");
        println!("Created user: {} (ID: {})", user.name(), user.id());
        user.id()
    };

    let user2_id = {
        let user = service.create_user("Bob", "bob@example.com");
        println!("Created user: {} (ID: {})", user.name(), user.id());
        user.id()
    };

    println!("Total users: {}", service.count());
    println!();

    // READ: Find users
    println!("=== Reading users ===");
    if let Some(found) = service.find_by_email("alice@example.com") {
        println!("Found user by email: {} ({})", found.name(), found.email());
    }

    if let Some(found) = service.find_by_id(user2_id) {
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
        email: None,
        role: None,
    };

    match service.update_user(user1_id, update_data) {
        Ok(_) => println!("User {} updated successfully", user1_id),
        Err(e) => println!("Update failed: {}", e),
    }

    if let Some(updated) = service.find_by_id(user1_id) {
        println!("Updated user: {} ({})", updated.name(), updated.email());
    }
    println!();

    // DELETE: Remove users
    println!("=== Deleting users ===");
    let removed = service.remove_by_id(user1_id);
    println!("Removed user ID {}: {}", user1_id, removed);

    let removed = service.remove_by_email("bob@example.com");
    println!("Removed user with email bob@example.com: {}", removed);

    println!("Remaining users: {}", service.count());
    println!("Service empty: {}", service.is_empty());
}
