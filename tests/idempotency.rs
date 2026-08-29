#[tokio::test]
async fn test_payment_idempotency() {
    let api_url = std::env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let client = reqwest::Client::new();
    let auth_header = "Bearer dodo_test_seed_key_abc123";
    let idempotency_key = "idempotency_test_key_999";

    // 1. Create a customer
    let customer_resp = client.post(format!("{}/v1/customers", api_url))
        .header("Authorization", auth_header)
        .json(&serde_json::json!({
            "name": "Idempotency Customer",
            "email": "idempotency@example.com"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(customer_resp.status(), 201);
    let customer_json: serde_json::Value = customer_resp.json().await.unwrap();
    let customer_id = customer_json["id"].as_str().unwrap();

    // 2. Create an invoice
    let invoice_resp = client.post(format!("{}/v1/invoices", api_url))
        .header("Authorization", auth_header)
        .json(&serde_json::json!({
            "customer_id": customer_id,
            "due_date": "2026-12-31",
            "items": [
                {
                    "description": "Integration Test Item",
                    "quantity": 1,
                    "unit_amount_cents": 5000
                }
            ]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(invoice_resp.status(), 201);
    let invoice_json: serde_json::Value = invoice_resp.json().await.unwrap();
    let invoice_id = invoice_json["id"].as_str().unwrap();

    // 3. First payment attempt (success token)
    let p1_resp = client.post(format!("{}/v1/invoices/{}/pay", api_url, invoice_id))
        .header("Authorization", auth_header)
        .header("Idempotency-Key", idempotency_key)
        .json(&serde_json::json!({
            "card_token": "tok_success"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(p1_resp.status(), 200);
    let p1_json: serde_json::Value = p1_resp.json().await.unwrap();
    assert_eq!(p1_json["status"].as_str().unwrap(), "succeeded");
    let p1_attempt_id = p1_json["id"].as_str().unwrap();

    // 4. Replay identical payment request
    let p2_resp = client.post(format!("{}/v1/invoices/{}/pay", api_url, invoice_id))
        .header("Authorization", auth_header)
        .header("Idempotency-Key", idempotency_key)
        .json(&serde_json::json!({
            "card_token": "tok_success"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(p2_resp.status(), 200);
    let p2_json: serde_json::Value = p2_resp.json().await.unwrap();
    assert_eq!(p2_json["status"].as_str().unwrap(), "succeeded");
    assert_eq!(p2_json["id"].as_str().unwrap(), p1_attempt_id);

    // 5. Replay with different body (expecting 409 Conflict)
    let p3_resp = client.post(format!("{}/v1/invoices/{}/pay", api_url, invoice_id))
        .header("Authorization", auth_header)
        .header("Idempotency-Key", idempotency_key)
        .json(&serde_json::json!({
            "card_token": "tok_card_declined" // modified
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(p3_resp.status(), 409);
    let p3_json: serde_json::Value = p3_resp.json().await.unwrap();
    assert_eq!(p3_json["error"]["code"].as_str().unwrap(), "idempotency_key_reused");
}
