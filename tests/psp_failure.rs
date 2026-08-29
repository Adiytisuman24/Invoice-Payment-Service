use std::time::Instant;

#[tokio::test]
async fn test_psp_failures_and_timeouts() {
    let api_url = std::env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let client = reqwest::Client::new();
    let auth_header = "Bearer dodo_test_seed_key_abc123";

    // 1. Create customer
    let customer_resp = client.post(format!("{}/v1/customers", api_url))
        .header("Authorization", auth_header)
        .json(&serde_json::json!({
            "name": "Failure Customer",
            "email": "failures@example.com"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(customer_resp.status(), 201);
    let customer_json: serde_json::Value = customer_resp.json().await.unwrap();
    let customer_id = customer_json["id"].as_str().unwrap();

    // --- CASE A: Card Declined ---
    let invoice_a = create_test_invoice(&client, &api_url, auth_header, customer_id).await;
    let p_declined_resp = client.post(format!("{}/v1/invoices/{}/pay", api_url, invoice_a))
        .header("Authorization", auth_header)
        .header("Idempotency-Key", "key_decline_1")
        .json(&serde_json::json!({
            "card_token": "tok_card_declined"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(p_declined_resp.status(), 200);
    let p_declined_json: serde_json::Value = p_declined_resp.json().await.unwrap();
    assert_eq!(p_declined_json["status"].as_str().unwrap(), "failed");
    assert_eq!(p_declined_json["failure_code"].as_str().unwrap(), "card_declined");

    // Verify invoice remains open
    assert_invoice_state(&client, &api_url, auth_header, &invoice_a, "open").await;

    // --- CASE B: Network Crash (tok_network_error) ---
    let invoice_b = create_test_invoice(&client, &api_url, auth_header, customer_id).await;
    let p_crash_resp = client.post(format!("{}/v1/invoices/{}/pay", api_url, invoice_b))
        .header("Authorization", auth_header)
        .header("Idempotency-Key", "key_crash_1")
        .json(&serde_json::json!({
            "card_token": "tok_network_error"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(p_crash_resp.status(), 200);
    let p_crash_json: serde_json::Value = p_crash_resp.json().await.unwrap();
    assert_eq!(p_crash_json["status"].as_str().unwrap(), "failed");
    assert_eq!(p_crash_json["failure_code"].as_str().unwrap(), "psp_unavailable");

    // Verify invoice remains open
    assert_invoice_state(&client, &api_url, auth_header, &invoice_b, "open").await;

    // --- CASE C: PSP Timeout (tok_timeout) ---
    let invoice_c = create_test_invoice(&client, &api_url, auth_header, customer_id).await;
    
    let start_time = Instant::now();
    let p_timeout_resp = client.post(format!("{}/v1/invoices/{}/pay", api_url, invoice_c))
        .header("Authorization", auth_header)
        .header("Idempotency-Key", "key_timeout_1")
        .json(&serde_json::json!({
            "card_token": "tok_timeout"
        }))
        .send()
        .await
        .unwrap();
    
    let duration = start_time.elapsed();
    
    // Server must respond 202 Accepted and NOT wait for the mock PSP's 30s timeout
    assert_eq!(p_timeout_resp.status(), 202);
    assert!(duration.as_secs() < 8, "API call should return within 5-second client timeout, took {}s", duration.as_secs());
    
    let p_timeout_json: serde_json::Value = p_timeout_resp.json().await.unwrap();
    assert_eq!(p_timeout_json["status"].as_str().unwrap(), "pending");

    // Verify invoice remains open
    assert_invoice_state(&client, &api_url, auth_header, &invoice_c, "open").await;
}

async fn create_test_invoice(client: &reqwest::Client, api_url: &str, auth: &str, customer_id: &str) -> String {
    let resp = client.post(format!("{}/v1/invoices", api_url))
        .header("Authorization", auth)
        .json(&serde_json::json!({
            "customer_id": customer_id,
            "due_date": "2026-12-31",
            "items": [
                {
                    "description": "Failures Test",
                    "quantity": 1,
                    "unit_amount_cents": 1000
                }
            ]
        }))
        .send()
        .await
        .unwrap();
    let json: serde_json::Value = resp.json().await.unwrap();
    json["id"].as_str().unwrap().to_string()
}

async fn assert_invoice_state(client: &reqwest::Client, api_url: &str, auth: &str, invoice_id: &str, expected_state: &str) {
    let resp = client.get(format!("{}/v1/invoices/{}", api_url, invoice_id))
        .header("Authorization", auth)
        .send()
        .await
        .unwrap();
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["state"].as_str().unwrap(), expected_state);
}
