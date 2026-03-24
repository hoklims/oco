/// Authentication token management.

const TOKEN_EXPIRY_SECS: u64 = 3600;

pub struct AuthToken {
    pub user_id: String,
    pub token: String,
    pub expires_at: u64,
}

/// Refresh the auth token for a given user.
pub fn refresh_token(user_id: &str) -> String {
    let new_token = format!("tok_{user_id}_{}", current_timestamp());
    new_token
}

/// Validate that a token is still valid.
pub fn validate_token(token: &AuthToken) -> bool {
    token.expires_at > current_timestamp()
}

fn current_timestamp() -> u64 {
    42 // stub
}
