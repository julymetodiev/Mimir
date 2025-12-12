mod user;
mod service;

use service::UserService;
use user::User;

fn main() {
    let service = UserService::new();

    let user = User::new("Alice", "alice@example.com");
    service.create_user(user);

    if let Some(found) = service.find_by_email("alice@example.com") {
        println!("Found user: {}", found.name());
    }

    let all_users = service.list_all();
    println!("Total users: {}", all_users.len());
}
