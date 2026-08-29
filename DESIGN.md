# Dodo Payments - System Design Document

This document outlines the architectural decisions, data models, state transition boundaries, and failure mitigations implemented in the Dodo Payments take-home service.

---

## 1. Data Model

The data model is implemented in PostgreSQL to ensure ACID transactions, strict schema enforcement, and row-level locking capabilities. 

Money fields are stored strictly as `BIGINT` representing minor units (integer cents) to avoid IEEE 754 floating-point rounding errors. 

```text
  ┌──────────────────┐
  │    businesses    │
  └────────┬─────────┘
           │ 1
           ├──────────────────────────────┐
           │ 1..*                         │ 1..*
  ┌────────▼─────────┐           ┌────────▼─────────┐
  │     api_keys     │           │    customers     │
  └──────────────────┘           └────────┬─────────┘
                                          │ 1
                                          │ 1..*
                                 ┌────────▼─────────┐
                                 │    invoices      ◄──────┐
                                 └────────┬─────────┘      │
                                          │ 1              │
                                          ├────────────────┤ 1..*
                                 ┌────────▼─────────┐ ┌────▼─────────────┐
                                 │  invoice_items   │ │ payment_attempts │
                                 └──────────────────┘ └──────────────────┘
```

### Critical Fields:
* **`api_keys.key_hash`:** The API key itself is never stored. Only a cryptographic hash (SHA-256) is recorded.
* **`invoices.total_cents`:** Automatically computed by the server as $\sum (\text{quantity} \times \text{unit\_amount\_cents})$ to prevent clients from submitting arbitrary totals.
* **`payment_attempts.idempotency_key`:** Scoped uniquely per invoice (`UNIQUE(invoice_id, idempotency_key)`), serving as the primary serialization boundary.

---

## 2. Invoice State Machine

The system enforces a clear boundary between the **Invoice state** and the **Payment Attempt state**.

```text
Invoice States:
  DRAFT ──[ finalize ]──► OPEN ──[ payment success ]──► PAID (Terminal)
                           │
                           ├──[ business void ]───────► VOID (Terminal)
                           │
                           └──[ mark default ]────────► UNCOLLECTIBLE (Terminal)

Payment Attempt States:
  PENDING ──┬──[ PSP Success ]──► SUCCEEDED (Invoice -> PAID)
            │
            ├──[ PSP Decline ]──► FAILED (Invoice remains OPEN)
            │
            └──[ Network Error ]► FAILED (Invoice remains OPEN)
```

Transitions from terminal states (`PAID`, `VOID`, `UNCOLLECTIBLE`) are strictly blocked at the database row lock stage. If a request attempts to change a terminal invoice state, it is rejected with a `409 Conflict` response.

---

## 3. Payment Correctness & Failure Modes

### Concurrent Payments
If multiple concurrent requests arrive at `/pay` for the same invoice, they are serialized using PostgreSQL row-level locks:
```sql
SELECT * FROM invoices WHERE id = $1 AND business_id = $2 FOR UPDATE;
```
1. **Transaction 1** locks the invoice row. All other threads block.
2. The winning thread validates the state (must be `OPEN`) and queries for existing active `PENDING` or `SUCCEEDED` payment attempts.
3. If none exist, it inserts a new `PENDING` payment attempt record.
4. **Transaction 1** commits, releasing the lock.
5. While the winner calls the PSP (outside the transaction), subsequent blocked requests acquire the lock sequentially. They query the state, find the active `PENDING` payment attempt, commit, and return `202 Accepted` immediately without calling the PSP.

This guarantees that **at most one thread** is communicating with the external PSP for a given invoice at any point in time, preventing double charges.

### PSP Timeout
In payment processing, a network timeout (the HTTP client times out before the server responds) is an **ambiguous outcome**. It does not prove that the card was not charged.
* **Mitigation:** We set a strict 5-second client-side timeout. If a timeout occurs, we **do not** mark the attempt as failed. It remains in the `pending` state in the database, and the invoice remains `open`. The API immediately returns a `202 Accepted` response.
* **Production Resolution:** To resolve this ambiguity, a production service would rely on webhook updates from the PSP or invoke a status check/reconciliation API to update the state of the pending attempt.

### PSP Success + Application Crash
If the PSP successfully charges the card, but the application crashes before persisting the result:
* **Mitigation:** We record the `attempt_id` inside our database *before* making the call. When calling the PSP, we pass this stable `attempt_id` as the external idempotency key. Upon application recovery or client replay, the PSP returns the cached success response rather than double-charging. This is the only way to achieve end-to-end replay safety across distributed boundaries.

### Idempotency Key Reuse
* **Same key + same body:** We return the previously cached payment response (either `200` with the result, or `202` if it is still pending).
* **Same key + different body:** The server computes a SHA-256 hash of the request payload and compares it to the stored hash. If they differ, the server rejects the request with `409 Conflict` (code `idempotency_key_reused`) to prevent request payload tampering.

### Payment on a Paid Invoice
The initial lock query checks if `invoice.state = 'paid'`. If so, the transaction is terminated and the request is rejected with `409 Conflict` (code `invoice_not_payable`).

---

## 4. Webhook Design

Webhook delivery is decoupled from the main HTTP thread to prevent slow client servers from blocking API response times:
1. **Enqueuing:** Webhook events are enqueued as rows in `webhook_deliveries` atomically inside the payment update transaction.
2. **Worker Polling:** A background Tokio task polls the table for pending items where `next_attempt_at <= now()`.
3. **HMAC-SHA256 Signing:** The body is signed using the endpoint's secret:
   $$\text{Signature} = \text{HMAC-SHA256}(\text{signing\_secret}, \text{timestamp} + \text{"."} + \text{payload})$$
   The signature is sent via the `X-Webhook-Signature` header, along with a `X-Webhook-Timestamp` for replay protection.
4. **Retry Policy:** We implement exponential backoff with a maximum of 5 attempts:
   * Attempt 1: Immediate
   * Attempt 2: +10 seconds
   * Attempt 3: +30 seconds
   * Attempt 4: +2 minutes
   * Attempt 5: +10 minutes
   * If all fail, the delivery is marked as `exhausted`.

---

## 5. API Key Model

We use a hashed API key model:
* **Prefix:** API keys are generated as `dodo_test_` followed by random characters. We store a short prefix (`dodo_test_...`) in plain text to assist in debugging.
* **Hash:** We store the SHA-256 hash of the key. When an API call arrives, the key is extracted from the `Authorization: Bearer <key>` header, hashed, and looked up in the database.
* **Revocation:** Revocation is supported via a `revoked_at` timestamp.
* **Isolation:** Every SQL query is scoped by the `business_id` derived from the validated API key.

---

## 6. What I Cut and Why

* **Refunds / Partial Payments:** Excluded as they represent separate domain requirements and were explicitly out-of-scope.
* **OAuth / OAuth2:** Replaced by straightforward, secure API keys, which are standard for developer-facing payment APIs.
* **Redis Queue:** Replaced by a PostgreSQL-backed queue table. Using Postgres avoids introducing another stateful dependency (Redis), keeping the project simple and self-contained while preserving transactional guarantees for webhook enqueuing.

---

## 7. Production Readiness Gap

1. **Database Connection Tuning:** The PgPool should be tuned for high availability, utilizing connection pooling (e.g., PgBouncer) and read replicas.
2. **Distributed Locking:** For horizontal API scaling, row locking (`FOR UPDATE`) is highly effective but holds database connections. In huge scale, one might employ Redis-based advisory locks (Redlock) or optimistic concurrency models to reduce DB lock hold times.
3. **API Key Management Service:** Key generation and hashing should be delegated to a KMS or vault-like service with rotation support, rather than simple DB insertions.
4. **Reconciliation Cron:** A daily cron job should poll the PSP for all `pending` payment attempts to resolve timed-out transactions.
5. **Rate Limiting:** IP-based and business-based token bucket rate limiting should be deployed at the API gateway layer to prevent denial-of-service.
