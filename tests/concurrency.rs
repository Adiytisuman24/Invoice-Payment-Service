#[tokio::test]
async fn test_concurrent_payments() {
    let api_url = std::env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let client = reqwest::Client::new();
    let auth_header = "Bearer dodo_test_seed_key_abc123";

    // 1. Create a customer
    let customer_resp = client.post(format!("{}/v1/customers", api_url))
        .header("Authorization", auth_header)
        .json(&serde_json::json!({
            "name": "Concurrency Customer",
            "email": "concurrency@example.com"
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
                    "unit_amount_cents": 10000
                }
            ]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(invoice_resp.status(), 201);
    let invoice_json: serde_json::Value = invoice_resp.json().await.unwrap();
    let invoice_id = invoice_json["id"].as_str().unwrap();

    // 3. Fire 10 concurrent requests with DIFFERENT idempotency keys
    let mut handles = Vec::new();
    for i in 0..10 {
        let client_clone = client.clone();
        let api_url_clone = api_url.clone();
        let invoice_id_clone = invoice_id.to_string();
        let idempotency_key = format!("idempotency_concurrency_{}", i);

        let handle = tokio::spawn(async move {
            client_clone.post(format!("{}/v1/invoices/{}/pay", api_url_clone, invoice_id_clone))
                .header("Authorization", auth_header)
                .header("Idempotency-Key", idempotency_key)
                .json(&serde_json::json!({
                    "card_token": "tok_success"
                }))
                .send()
                .await
        });
        handles.push(handle);
    }

    let mut succeeded_count = 0;
    let mut pending_count = 0;

    for handle in handles {
        let resp = handle.await.unwrap().unwrap();
        let status = resp.status();
        let body: serde_json::Value = resp.json().await.unwrap();
        let attempt_status = body["status"].as_str().unwrap();

        if status == 200 && attempt_status == "succeeded" {
            succeeded_count += 1;
        } else if status == 202 && attempt_status == "pending" {
            pending_count += 1;
        } else {
            panic!("Unexpected response status: {} body: {:?}", status, body);
        }
    }

    // Exactly 1 request must have succeeded, and the other 9 must be pending (since they hit the lock during the active charge)
    assert_eq!(succeeded_count, 1, "Exactly one request must succeed");
    assert_eq!(pending_count, 9, "The other nine requests must be marked pending");

    // 4. Retrieve invoice details and verify the state is paid
    let get_resp = client.get(format!("{}/v1/invoices/{}", api_url, invoice_id))
        .header("Authorization", auth_header)
        .send()
        .await
        .unwrap();
    assert_eq!(get_resp.status(), 200);
    let final_invoice: serde_json::Value = get_resp.json().await.unwrap();
    assert_eq!(final_invoice["state"].as_str().unwrap(), "paid");
}
