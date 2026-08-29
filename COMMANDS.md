# Dodo Payments — Live Testing Commands & Cheatsheet

```text
Default API Key:  dodo_test_seed_key_abc123
Invoice API URL:  http://localhost:8080
Mock PSP URL:     http://localhost:8081
Database URL:     postgres://postgres:postgres@localhost:5432/dodo
```

---

## 0. Start / Stop Services

```bash
# Start all services, wait for readiness, and run all 14 automated tests:
./dodopayments.sh test

# Start services in background without running tests:
./dodopayments.sh

# Stop all containers:
docker compose down

# Stop containers and wipe PostgreSQL volume:
docker compose down -v
```

---

## 1. Authentication

```bash
# Missing or malformed header → HTTP 401 Unauthorized
curl -s http://localhost:8080/v1/customers | python3 -m json.tool

# Valid Bearer token → HTTP 200 OK
curl -s -H "Authorization: Bearer dodo_test_seed_key_abc123" \
     http://localhost:8080/v1/customers | python3 -m json.tool
```

---

## 2. Customers

```bash
# Create a new customer
curl -s -X POST http://localhost:8080/v1/customers \
  -H "Authorization: Bearer dodo_test_seed_key_abc123" \
  -H "Content-Type: application/json" \
  -d '{"name":"Alice Smith","email":"alice@example.com"}' \
  | python3 -m json.tool

# List all customers for this business
curl -s http://localhost:8080/v1/customers \
  -H "Authorization: Bearer dodo_test_seed_key_abc123" \
  | python3 -m json.tool

# Retrieve a specific customer by UUID
curl -s http://localhost:8080/v1/customers/<CUSTOMER_UUID> \
  -H "Authorization: Bearer dodo_test_seed_key_abc123" \
  | python3 -m json.tool
```

---

## 3. Invoices

```bash
# Create an invoice with line items (server computes total_cents automatically)
curl -s -X POST http://localhost:8080/v1/invoices \
  -H "Authorization: Bearer dodo_test_seed_key_abc123" \
  -H "Content-Type: application/json" \
  -d '{
    "customer_id": "<CUSTOMER_UUID>",
    "due_date": "2026-12-31",
    "items": [
      {"description": "Pro Monthly Plan", "quantity": 1, "unit_amount_cents": 4999},
      {"description": "API Add-on",        "quantity": 2, "unit_amount_cents": 1500}
    ]
  }' | python3 -m json.tool

# List all invoices
curl -s http://localhost:8080/v1/invoices \
  -H "Authorization: Bearer dodo_test_seed_key_abc123" \
  | python3 -m json.tool

# Retrieve a single invoice
curl -s http://localhost:8080/v1/invoices/<INVOICE_UUID> \
  -H "Authorization: Bearer dodo_test_seed_key_abc123" \
  | python3 -m json.tool

# Void an open invoice
curl -s -X POST http://localhost:8080/v1/invoices/<INVOICE_UUID>/void \
  -H "Authorization: Bearer dodo_test_seed_key_abc123" \
  | python3 -m json.tool
```

---

## 4. Payment Operations

```bash
# 1. Successful payment (transitions invoice state to 'paid')
curl -s -X POST http://localhost:8080/v1/invoices/<INVOICE_UUID>/pay \
  -H "Authorization: Bearer dodo_test_seed_key_abc123" \
  -H "Content-Type: application/json" \
  -H "Idempotency-Key: pay-key-001" \
  -d '{"card_token": "tok_success"}' \
  | python3 -m json.tool

# 2. Card declined (invoice remains 'open')
curl -s -X POST http://localhost:8080/v1/invoices/<INVOICE_UUID>/pay \
  -H "Authorization: Bearer dodo_test_seed_key_abc123" \
  -H "Content-Type: application/json" \
  -H "Idempotency-Key: pay-key-002" \
  -d '{"card_token": "tok_card_declined"}' \
  | python3 -m json.tool

# 3. PSP Timeout (client times out in 5s vs 30s PSP sleep -> returns 202 Accepted)
time curl -s -X POST http://localhost:8080/v1/invoices/<INVOICE_UUID>/pay \
  -H "Authorization: Bearer dodo_test_seed_key_abc123" \
  -H "Content-Type: application/json" \
  -H "Idempotency-Key: pay-key-003" \
  -d '{"card_token": "tok_timeout"}' \
  | python3 -m json.tool

# 4. Idempotency Replay (same key + same body -> stored response returned verbatim)
curl -s -X POST http://localhost:8080/v1/invoices/<INVOICE_UUID>/pay \
  -H "Authorization: Bearer dodo_test_seed_key_abc123" \
  -H "Content-Type: application/json" \
  -H "Idempotency-Key: pay-key-001" \
  -d '{"card_token": "tok_success"}' \
  | python3 -m json.tool

# 5. Idempotency Conflict (same key + different body -> HTTP 409 Conflict)
curl -s -X POST http://localhost:8080/v1/invoices/<INVOICE_UUID>/pay \
  -H "Authorization: Bearer dodo_test_seed_key_abc123" \
  -H "Content-Type: application/json" \
  -H "Idempotency-Key: pay-key-001" \
  -d '{"card_token": "tok_card_declined"}' \
  | python3 -m json.tool
```

### Supported Card Tokens
| Token | Mock PSP Action | Outcome |
|-------|-----------------|---------|
| `tok_success` | Returns `succeeded` with `psp_ref` | Invoice marked `paid`, `invoice.paid` webhook queued |
| `tok_card_declined` | Returns `failed` (`card_declined`) | Invoice stays `open`, `invoice.payment_failed` queued |
| `tok_insufficient_funds` | Returns `failed` (`insufficient_funds`) | Invoice stays `open` |
| `tok_network_error` | Simulates immediate 500 error | Invoice stays `open` |
| `tok_timeout` | Sleeps 30s | API client times out in 5s, returns `202 Accepted` (`pending`) |

---

## 5. Webhooks

```bash
# Register webhook endpoint (signing_secret returned ONCE here)
curl -s -X POST http://localhost:8080/v1/webhook-endpoints \
  -H "Authorization: Bearer dodo_test_seed_key_abc123" \
  -H "Content-Type: application/json" \
  -d '{"url": "https://httpbin.org/post"}' \
  | python3 -m json.tool

# List webhook endpoints (signing_secret is REDACTED)
curl -s http://localhost:8080/v1/webhook-endpoints \
  -H "Authorization: Bearer dodo_test_seed_key_abc123" \
  | python3 -m json.tool
```

---

## 6. Mock PSP (Direct Port 8081)

```bash
# Direct success call
curl -s -X POST http://localhost:8081/payments \
  -H "Content-Type: application/json" \
  -d '{"card_token":"tok_success","idempotency_key":"direct-001"}' \
  | python3 -m json.tool

# Direct decline call
curl -s -X POST http://localhost:8081/payments \
  -H "Content-Type: application/json" \
  -d '{"card_token":"tok_card_declined","idempotency_key":"direct-002"}' \
  | python3 -m json.tool
```

---

## 7. PostgreSQL Database Queries

```bash
# Connect interactively to PostgreSQL
docker exec -it dodo-postgres psql -U postgres -d dodo

# Check row counts across all 7 domain tables:
docker exec dodo-postgres psql -U postgres -d dodo -c \
  "SELECT tbl, rows FROM (
     SELECT 'businesses'          AS tbl, COUNT(*) AS rows FROM businesses        UNION ALL
     SELECT 'customers',                  COUNT(*) FROM customers                  UNION ALL
     SELECT 'invoices',                   COUNT(*) FROM invoices                   UNION ALL
     SELECT 'invoice_items',              COUNT(*) FROM invoice_items              UNION ALL
     SELECT 'payment_attempts',           COUNT(*) FROM payment_attempts           UNION ALL
     SELECT 'webhook_endpoints',          COUNT(*) FROM webhook_endpoints          UNION ALL
     SELECT 'webhook_deliveries',         COUNT(*) FROM webhook_deliveries
   ) x ORDER BY tbl;"

# View recent invoices and current states:
docker exec dodo-postgres psql -U postgres -d dodo -c \
  "SELECT id, state, total_cents, created_at FROM invoices ORDER BY created_at DESC LIMIT 10;"

# View recent payment attempts and recorded PSP references:
docker exec dodo-postgres psql -U postgres -d dodo -c \
  "SELECT id, invoice_id, status, failure_code, psp_ref FROM payment_attempts ORDER BY created_at DESC LIMIT 10;"

# View webhook queue and delivery status:
docker exec dodo-postgres psql -U postgres -d dodo -c \
  "SELECT id, event_type, status, attempts, next_attempt_at FROM webhook_deliveries ORDER BY created_at DESC LIMIT 10;"
```

---

## 8. Integration Tests (Cargo)

```bash
# Run all 4 integration test suites:
DATABASE_URL="postgres://postgres:postgres@localhost:5432/dodo" \
API_URL="http://localhost:8080" \
cargo test -- --nocapture
```
