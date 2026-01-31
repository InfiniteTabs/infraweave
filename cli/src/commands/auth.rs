use anyhow::{anyhow, Context, Result};
use aws_credential_types::provider::ProvideCredentials;
use colored::Colorize;
use env_utils::config_path::get_token_path;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;

const CALLBACK_PORT: u16 = 8080;
const CALLBACK_PATH: &str = "/callback";

#[derive(Debug, Serialize)]
struct TokenRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    redirect_uri: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    sign_in_url: Option<String>,
    access_token: Option<String>,
    id_token: Option<String>,
    refresh_token: Option<String>,
    token_type: Option<String>,
    expires_in: Option<u64>,
    #[serde(default = "default_true")]
    success: bool,
    message: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredTokens {
    access_token: String,
    id_token: String,
    refresh_token: Option<String>,
    expires_at: Option<i64>,
    api_endpoint: String,
}

/// Execute login flow: get sign-in URL, open browser, catch callback, exchange code for tokens
pub async fn execute_login(api_endpoint: String) -> Result<()> {
    println!("{}", "Starting OAuth login flow...".bright_blue());

    // Step 1: Get the sign-in URL from the API
    println!("Requesting sign-in URL...");
    let client = reqwest::Client::new();

    let token_url = format!("{}/api/v1/auth/token", api_endpoint.trim_end_matches('/'));

    // Get AWS credentials for IAM signing
    let aws_config = aws_config::load_from_env().await;
    let credentials = aws_config
        .credentials_provider()
        .ok_or_else(|| anyhow!("No AWS credentials found"))?
        .provide_credentials()
        .await
        .context("Failed to get AWS credentials")?;

    let mut region = aws_config
        .region()
        .map(|r| r.as_ref().to_string())
        .unwrap_or_else(|| "us-west-2".to_string());

    // Always attempt to discover region from the API metadata
    // This supports both standard regional endpoints and multi-region custom domains
    println!(
        "{}",
        "Discovering API region from metadata endpoint...".dimmed()
    );
    let meta_url = format!("{}/api/v1/meta", api_endpoint.trim_end_matches('/'));

    // NOTE: The /meta endpoint MUST be unauthenticated because we cannot sign the request
    // until we know which region we are talking to (bootstrap problem).
    match client
        .get(&meta_url)
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
    {
        Ok(resp) => {
            if resp.status().is_success() {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    if let Some(r) = json.get("region").and_then(|v| v.as_str()) {
                        if r != region {
                            println!(
                                "{}",
                                format!("✓ Discovered region '{}' from metadata", r).green()
                            );
                            region = r.to_string();
                        } else {
                            println!(
                                "{}",
                                format!("✓ Confirmed region '{}' from metadata", r).green()
                            );
                        }
                    }
                }
            } else {
                println!(
                    "{}",
                    format!(
                        "Warning: Metadata endpoint returned status {}. Proceeding with configured region: {}",
                        resp.status(),
                        region
                    )
                    .yellow()
                );
            }
        }
        Err(e) => {
            println!(
                "{}",
                format!(
                    "Warning: Failed to discover region metadata ({}). Proceeding with configured region: {}",
                    e, region
                )
                .yellow()
            );
        }
    }

    // Make the first request to get the sign-in URL
    let response = sign_request(&client, &token_url, &credentials, &region, None, None)
        .await
        .context("Failed to request sign-in URL")?;

    // Check HTTP status first
    let status = response.status();
    let response_text = response
        .text()
        .await
        .context("Failed to read response body")?;

    if !status.is_success() {
        println!("Type: {}", status);
        println!("Region: {}", region);
        return Err(anyhow!(
            "API request failed with status {}: {}",
            status,
            response_text
        ));
    }

    let token_response: TokenResponse = serde_json::from_str(&response_text).context(format!(
        "Failed to parse sign-in URL response. Response body: {}",
        response_text
    ))?;

    if !token_response.success {
        return Err(anyhow!(
            "Failed to get sign-in URL: {}",
            token_response.message.unwrap_or_default()
        ));
    }

    let sign_in_url = token_response
        .sign_in_url
        .ok_or_else(|| anyhow!("No sign-in URL in response"))?;

    println!("{} {}", "Sign-in URL:".green(), sign_in_url);

    // Step 2: Start local callback server
    let (tx, rx) = mpsc::channel();
    let listener = TcpListener::bind(format!("127.0.0.1:{}", CALLBACK_PORT)).context(format!(
        "Failed to bind to localhost:{}. Is another instance running?",
        CALLBACK_PORT
    ))?;

    println!(
        "{}",
        format!(
            "Starting local server on http://localhost:{}...",
            CALLBACK_PORT
        )
        .bright_blue()
    );

    // Spawn server thread
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let buf_reader = BufReader::new(&stream);
            let request_line = buf_reader.lines().next();

            if let Some(Ok(line)) = request_line {
                // Parse the request line to extract the code
                if let Some(query_start) = line.find("?") {
                    if let Some(http_start) = line.find(" HTTP/") {
                        let query = &line[query_start + 1..http_start];
                        for param in query.split('&') {
                            if let Some((key, value)) = param.split_once('=') {
                                if key == "code" {
                                    // Send success response to browser
                                    let response = "HTTP/1.1 200 OK\r\n\r\n<html><body><h1>Login Successful!</h1><p>You can close this window and return to the CLI.</p></body></html>";
                                    let _ = stream.write_all(response.as_bytes());
                                    let _ = tx.send(value.to_string());
                                    return;
                                }
                            }
                        }
                    }
                }
                // Send error response if no code found
                let response = "HTTP/1.1 400 Bad Request\r\n\r\n<html><body><h1>Error</h1><p>No authorization code found.</p></body></html>";
                let _ = stream.write_all(response.as_bytes());
            }
        }
    });

    // Step 3: Open browser
    println!("{}", "Opening browser...".bright_blue());
    if let Err(e) = webbrowser::open(&sign_in_url) {
        eprintln!(
            "{} {}",
            "Warning: Could not open browser automatically:".yellow(),
            e
        );
        println!("\n{}", "Please open this URL manually:".yellow());
        println!("{}\n", sign_in_url.bright_cyan());
    }

    // Step 4: Wait for callback
    println!("{}", "Waiting for authentication callback...".bright_blue());
    let code = rx
        .recv_timeout(std::time::Duration::from_secs(300))
        .context("Timeout waiting for authentication (5 minutes)")?;

    println!("{}", "✓ Authorization code received".green());

    // Step 5: Exchange code for tokens
    println!("Exchanging code for tokens...");
    let redirect_uri = format!("http://localhost:{}{}", CALLBACK_PORT, CALLBACK_PATH);

    let response = sign_request(
        &client,
        &token_url,
        &credentials,
        &region,
        Some(code),
        Some(redirect_uri),
    )
    .await
    .context("Failed to exchange code for tokens")?;

    let status = response.status();
    let response_text = response
        .text()
        .await
        .context("Failed to read token response body")?;

    if !status.is_success() {
        return Err(anyhow!(
            "Token exchange failed with status {}: {}",
            status,
            response_text
        ));
    }

    let token_response: TokenResponse = serde_json::from_str(&response_text).context(format!(
        "Failed to parse token response. Response body: {}",
        response_text
    ))?;

    if !token_response.success {
        return Err(anyhow!(
            "Failed to exchange code for tokens: {}",
            token_response.message.unwrap_or_default()
        ));
    }

    let access_token = token_response
        .access_token
        .ok_or_else(|| anyhow!("No access token in response"))?;
    let id_token = token_response
        .id_token
        .ok_or_else(|| anyhow!("No ID token in response"))?;

    // Parse JWT to get expiration time
    let expires_at = extract_jwt_expiration(&access_token).ok();

    // Step 6: Store tokens
    let tokens = StoredTokens {
        access_token: access_token.clone(),
        id_token: id_token.clone(),
        refresh_token: token_response.refresh_token,
        expires_at,
        api_endpoint: api_endpoint.clone(),
    };

    store_tokens(&tokens)?;

    println!("{}", "✓ Login successful! Tokens stored.".green());
    println!(
        "{}",
        format!("Token file: {}", get_token_path()?.display()).dimmed()
    );
    println!(
        "{}",
        format!("API endpoint configured: {}", api_endpoint).dimmed()
    );

    Ok(())
}

/// Sign request with AWS SigV4 and send it
async fn sign_request(
    client: &reqwest::Client,
    url: &str,
    credentials: &aws_credential_types::Credentials,
    region: &str,
    code: Option<String>,
    redirect_uri: Option<String>,
) -> Result<reqwest::Response> {
    use aws_sigv4::http_request::{sign, SignableBody, SignableRequest, SigningSettings};
    use aws_sigv4::sign::v4;

    let mut body_json = serde_json::json!({});
    if let Some(code) = code {
        body_json["code"] = serde_json::Value::String(code);
    }
    if let Some(redirect_uri) = redirect_uri {
        body_json["redirect_uri"] = serde_json::Value::String(redirect_uri);
    }

    let body = serde_json::to_string(&body_json)?;

    // Convert credentials to Identity for signing
    let identity =
        aws_smithy_runtime_api::client::identity::Identity::new(credentials.clone(), None);

    // Create signing params
    let signing_settings = SigningSettings::default();
    let signing_params = v4::SigningParams::builder()
        .identity(&identity)
        .region(region)
        .name("execute-api")
        .time(std::time::SystemTime::now())
        .settings(signing_settings)
        .build()
        .map_err(|e| anyhow!("Failed to build signing params: {}", e))?;

    // Create signable request
    let signable_request = SignableRequest::new(
        "POST",
        url,
        std::iter::empty::<(&str, &str)>(),
        SignableBody::Bytes(body.as_bytes()),
    )
    .map_err(|e| anyhow!("Failed to create signable request: {}", e))?;

    // Sign the request
    let (signing_instructions, _signature) = sign(signable_request, &signing_params.into())
        .map_err(|e| anyhow!("Failed to sign request: {}", e))?
        .into_parts();

    // Build the actual HTTP request with signed headers
    let mut request = client.post(url);

    for (name, value) in signing_instructions.headers() {
        request = request.header(name, value);
    }

    // Set content-type after signing headers
    request = request.header("content-type", "application/json");

    if !body.is_empty() {
        request = request.body(body);
    }

    // Send the request
    let response = request.send().await?;
    Ok(response)
}

/// Store tokens to disk
fn store_tokens(tokens: &StoredTokens) -> Result<()> {
    let path = env_utils::config_path::get_token_path()?;
    let json = serde_json::to_string_pretty(tokens)?;
    std::fs::write(&path, json).context("Failed to write tokens to file")?;
    Ok(())
}

/// Load tokens from disk
pub fn load_tokens() -> Result<StoredTokens> {
    let path = env_utils::config_path::get_token_path()?;
    let json = std::fs::read_to_string(&path).context("Failed to read tokens file")?;
    let tokens: StoredTokens = serde_json::from_str(&json).context("Failed to parse tokens")?;
    Ok(tokens)
}

/// Check if user is logged in (has valid tokens)
pub fn is_logged_in() -> bool {
    load_tokens().is_ok()
}

/// Get the current access token (for use in API calls)
pub fn get_access_token() -> Result<String> {
    let tokens = load_tokens()?;
    // TODO: Check expiration and refresh if needed
    Ok(tokens.id_token)
}
/// Extract expiration time from JWT token
fn extract_jwt_expiration(token: &str) -> Result<i64> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    // JWT format: header.payload.signature
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(anyhow!("Invalid JWT format"));
    }

    // Decode the payload (second part) - JWTs use URL-safe base64 without padding
    let payload_bytes = URL_SAFE_NO_PAD
        .decode(parts[1])
        .context("Failed to decode JWT payload")?;
    let payload_str = String::from_utf8(payload_bytes).context("JWT payload is not valid UTF-8")?;
    let payload: serde_json::Value =
        serde_json::from_str(&payload_str).context("Failed to parse JWT payload JSON")?;

    // Extract expiration time
    payload
        .get("exp")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow!("No exp field in JWT"))
}
