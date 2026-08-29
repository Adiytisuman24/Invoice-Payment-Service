# Dodo Payments — Invoice & Payment Service

A small, deliberately correct invoice and payment service. The focus is on **payment correctness, concurrency safety, idempotency, PSP failure handling, and asynchronous webhook delivery** — not on feature breadth.

---

## Architecture

```text
Client
   │
   │ Authorization: Bearer <api-key>
   ▼
┌─────────────────────────────┐
│     Axum Invoice API        │  :8080
└──────┬──────────┬───────────┘
       │          │
       ▼          ▼
┌────────────┐  ┌──────────────┐
│ PostgreSQL │  │  Mock PSP    │  :8081
└──────┬─────┘  └──────────────┘
       │
       ▼
┌────────────────────────┐
│   Webhook Worker       │
│   HMAC-SHA256 signing  │
│   bounded retry backoff│
└──────────┬─────────────┘
           │ HTTP POST
           ▼
     Business webhook URL
```

---

## Tech Stack

| Layer            | Technology                        |
|------------------|-----------------------------------|
| Language         | Rust                              |
| HTTP framework   | Axum 0.7                          |
| Async runtime    | Tokio                             |
| Database         | PostgreSQL 16 + SQLx 0.8          |
| PSP              | Custom mock (second Rust binary)  |
| Containerisation | Docker Compose                    |

---

## Invoice State Machine

```text
OPEN ──[ payment success ]──► PAID ┐
  │                                │ terminal
  ├──[ void ]───────────────► VOID ┤
  │                                │
  └──[ mark uncollectible ]─► UNCOLLECTIBLE┘
```

`PAID`, `VOID`, and `UNCOLLECTIBLE` are terminal. Any request that attempts a transition out of a terminal state is rejected with `409 Conflict`. See [`DESIGN.md`](DESIGN.md) for the complete transition diagram and failure-mode analysis.

---

## Prerequisites

- [Docker & Docker Compose](https://docs.docker.com/get-docker/)
- [Rust & Cargo](https://rustup.rs/) (only needed to run tests locally)

---

## Getting Started

### Option A: Single-Prompt Runner & Test Suite (Recommended)

```bash
# Start all containers, wait for readiness, and run all 14 end-to-end verification tests:
./dodopayments.sh test

# Or simply start the services in background:
./dodopayments.sh
```

### Option B: Docker Compose

```bash
docker compose up --build -d
```

**Services:**
- Invoice API → `http://localhost:8080`
- Mock PSP → `http://localhost:8081`
- PostgreSQL → `localhost:5432`

No manual setup steps. Migrations run automatically at startup. For a full list of copy-paste test commands, see [`COMMANDS.md`](COMMANDS.md).

---

## Default Demo Credentials

Seeded at first startup (configurable via environment variables):

| Field        | Value                          |
|--------------|--------------------------------|
| API Key      | `dodo_test_seed_key_abc123`    |
| Business ID  | `00000000-0000-0000-0000-000000000001` |

---

## API Lifecycle — Curl Examples

### 1. Create a Customer

```bash
curl -s -X POST http://localhost:8080/v1/customers \
  -H "Authorization: Bearer dodo_test_seed_key_abc123" \
  -H "Content-Type: application/json" \
  -d '{"name": "Alice Smith", "email": "alice@example.com"}' | python3 -m json.tool
```

Copy the returned `"id"` — you need it for the next step.

### 2. Create an Invoice

```bash
curl -s -X POST http://localhost:8080/v1/invoices \
  -H "Authorization: Bearer dodo_test_seed_key_abc123" \
  -H "Content-Type: application/json" \
  -d '{
    "customer_id": "<CUSTOMER_UUID>",
    "due_date": "2026-12-31",
    "items": [
      {"description": "Engineering consulting", "quantity": 2, "unit_amount_cents": 50000},
      {"description": "Hosting fees",           "quantity": 1, "unit_amount_cents": 2500}
    ]
  }' | python3 -m json.tool
```

The server computes `total_cents = 102500` (i.e. $1,025.00). No client-provided total is accepted.

### 3. Successful Payment

```bash
curl -s -X POST http://localhost:8080/v1/invoices/<INVOICE_UUID>/pay \
  -H "Authorization: Bearer dodo_test_seed_key_abc123" \
  -H "Idempotency-Key: pay-demo-001" \
  -H "Content-Type: application/json" \
  -d '{"card_token": "tok_success"}' | python3 -m json.tool
```

Returns `200 OK` with `"status": "succeeded"`. Replaying the identical request with the same `Idempotency-Key` returns the **stored response verbatim** — no second PSP call is made.

### 4. Failed Payment (card declined)

```bash
curl -s -X POST http://localhost:8080/v1/invoices/<NEW_INVOICE_UUID>/pay \
  -H "Authorization: Bearer dodo_test_seed_key_abc123" \
  -H "Idempotency-Key: pay-demo-002" \
  -H "Content-Type: application/json" \
  -d '{"card_token": "tok_card_declined"}' | python3 -m json.tool
```

Returns `200 OK` with `"status": "failed"`, `"failure_code": "card_declined"`. The invoice remains `open` and can be retried.

### 5. Webhook Endpoint Registration

```bash
curl -s -X POST http://localhost:8080/v1/webhook-endpoints \
  -H "Authorization: Bearer dodo_test_seed_key_abc123" \
  -H "Content-Type: application/json" \
  -d '{"url": "https://httpbin.org/post"}' | python3 -m json.tool
```

> **Note:** `signing_secret` is returned **only once** upon registration. Subsequent `GET /v1/webhook-endpoints` requests return redacted records for security.

---

## Payment Token Behaviours (Mock PSP)

| Token                  | PSP latency | Invoice result | API response             |
|------------------------|-------------|----------------|--------------------------|
| `tok_success`          | ~100 ms     | `paid`         | `200 {"status":"succeeded"}` |
| `tok_card_declined`    | ~100 ms     | `open`         | `200 {"status":"failed","failure_code":"card_declined"}` |
| `tok_insufficient_funds` | ~100 ms   | `open`         | `200 {"status":"failed","failure_code":"insufficient_funds"}` |
| `tok_network_error`    | immediate   | `open`         | `200 {"status":"failed","failure_code":"psp_unavailable"}` |
| `tok_timeout`          | **30 s** (PSP) | `open`      | **`202 {"status":"pending"}`** in ~5 s |

### Timeout behaviour

The mock PSP deliberately sleeps **30 seconds** for `tok_timeout`. The service enforces a **5-second client-side HTTP timeout**. Verify with `time`:

```bash
time curl -s -X POST http://localhost:8080/v1/invoices/<INVOICE_UUID>/pay \
  -H "Authorization: Bearer dodo_test_seed_key_abc123" \
  -H "Idempotency-Key: pay-timeout-001" \
  -H "Content-Type: application/json" \
  -d '{"card_token": "tok_timeout"}' | python3 -m json.tool
```

Expected: `HTTP 202`, `"status": "pending"` in **≈5 seconds**, not 30. The invoice stays `open`. The attempt is `pending` because a timeout is an ambiguous outcome — the card may or may not have been charged. See [`DESIGN.md`](DESIGN.md) §3 for the full reasoning.

---

## Idempotency

Every `POST /v1/invoices/:id/pay` requires an `Idempotency-Key` header.

| Scenario | Result |
|----------|--------|
| Same key + same body | Stored response returned verbatim. No PSP call. |
| Same key + different body | `409 idempotency_key_reused` |
| Different key | New attempt created |

The response body and HTTP status code are stored in the database on first completion and replayed on subsequent retries — the service does not reconstruct the response from state, it returns the exact stored payload.

---

## Webhook Delivery

Events dispatched:
- `invoice.paid`
- `invoice.payment_failed`

Retry schedule (bounded backoff):

| Attempt | Delay after previous failure |
|---------|------------------------------|
| 1 | Immediate |
| 2 | +10 seconds |
| 3 | +30 seconds |
| 4 | +2 minutes |
| 5 | +10 minutes |
| — | `exhausted` (no further attempts) |

Each delivery is signed:
```
X-Webhook-Signature: sha256=<HMAC-SHA256(secret, timestamp + "." + body)>
X-Webhook-Timestamp: <unix timestamp>
X-Webhook-Id: <event UUID>
```

Deliveries are claimed using PostgreSQL `FOR UPDATE OF d SKIP LOCKED` inside an atomic transaction, marking claimed rows as `'processing'` to prevent concurrent worker duplication.

---

## Running Integration Tests

All integration tests run against the live stack (real HTTP, real PostgreSQL, real Mock PSP):

```bash
# 1. Ensure the stack is up
docker compose up -d

# 2. Run all integration tests
DATABASE_URL="postgres://postgres:postgres@localhost:5432/dodo" \
API_URL="http://localhost:8080" \
cargo test -- --nocapture
```

| Test | What it verifies |
|------|-----------------|
| `concurrency` | 10 concurrent `/pay` requests → exactly 1 PSP charge; remaining 9 requests serialized safely |
| `idempotency` | Replay returns stored response; reuse with different body → 409 Conflict |
| `payment_vs_void` | Concurrent payment + void race → re-locks invoice in Tx2, prevents split terminal state |
| `psp_failure` | Decline → failed+open; network error → failed+open; timeout → 202 pending in ≤8 s |

---

## Documentation

| File | Contents |
|------|----------|
| [`DESIGN.md`](DESIGN.md) | Data model, state machine, concurrency, two-lock race resolution, webhook atomicity, production gaps |
| [`AI_USAGE.md`](AI_USAGE.md) | Tools used, independent decisions, AI errors corrected |
| [`COMMANDS.md`](COMMANDS.md) | Copy-paste cheatsheet for testing every endpoint live |
| [`openapi.yaml`](openapi.yaml) | Full OpenAPI 3.0 specification |

---

## Demo Video

> 📹 **[Link to be added before submission]**

Structure (≈8 minutes):
1. `0:00–1:30` — Architecture walkthrough
2. `1:30–4:00` — Live `docker compose up` demo (customer → invoice → pay → webhook)
3. `4:00–6:00` — `DESIGN.md` state machine explanation
4. `6:00–8:00` — Payment engine code: `SELECT FOR UPDATE`, idempotency, `tok_timeout` → `pending`
