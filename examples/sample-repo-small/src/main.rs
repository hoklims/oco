/// Sample repo for testing OCO indexing and retrieval.
mod auth;
mod cache;

fn main() {
    let token = auth::refresh_token("user_123");
    println!("Token: {token}");

    let value = cache::get("key_1");
    println!("Cache: {value:?}");
}
