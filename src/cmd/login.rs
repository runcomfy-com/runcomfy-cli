use anyhow::Result;

use crate::config;

/// Trigger device-code OAuth flow. Opens browser to the verify URL,
/// polls the web auth endpoint until token is granted, then writes
/// the token to ~/.config/runcomfy/token.
pub async fn run(_web_base: Option<String>) -> Result<()> {
    // TODO: implement device-code flow
    //   1. POST {web_base}/api/cli-auth/start  → device_code, user_code, verify_url
    //   2. open(verify_url)
    //   3. poll {web_base}/api/cli-auth/poll  every 2s until token | denied | timeout
    //   4. config::save_token(token)
    println!("login — not implemented yet");
    Ok(())
}

pub async fn logout() -> Result<()> {
    config::clear_token()?;
    println!("Logged out.");
    Ok(())
}
