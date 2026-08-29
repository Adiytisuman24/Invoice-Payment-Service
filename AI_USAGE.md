# AI Usage Disclosure

This document discloses the use of AI assistants in the design and implementation of the Dodo Payments service, highlight independent engineering decisions, and details correction of AI errors.

---

## 1. Tools Used
- **Antigravity Coding Assistant (Gemini 3.5 Flash):** Used for scaffolding boilerplates, generating SQLx query statements, generating initial migration templates, and reviewing state machine logic.

---

## 2. Decisions Made Independently
The following structural decisions were made independently or against initial generic AI suggestions:

1. **PostgreSQL Row-level Locking (`SELECT ... FOR UPDATE`):**
   - *AI Suggestion:* Initial suggestions included optimistic locking or Redis-based distributed locks.
   - *Independent Choice:* I opted for native PostgreSQL row-level locks (`SELECT FOR UPDATE`) on the invoice. Since the invoice row is the natural transaction boundary, database-level locking is the most bulletproof way to prevent concurrent payment racing, without introducing external distributed state.
2. **Pending on PSP Timeout:**
   - *AI Suggestion:* Initial code templates marked payment attempts as `failed` when the HTTP request to the PSP timed out.
   - *Independent Choice:* I overrode this to keep the attempt in the `pending` state and the invoice in the `open` state. A client-side HTTP timeout represents an *ambiguous* outcome where the card could still be charged. Marking it failed immediately risks double-charging the customer on a subsequent retry.
3. **PostgreSQL-backed Webhook Queue:**
   - *AI Suggestion:* The AI initially suggested spinning up a Redis instance and using a library like `celery` or `sidekiq` style queues for processing webhooks.
   - *Independent Choice:* I decided to use a dedicated database table (`webhook_deliveries`) as our queue. This preserves strict transactional guarantees (we enqueue the webhook event in the same transaction that updates the payment state) and eliminates the need to run and manage Redis in our Docker Compose stack.

---

## 3. AI Errors Corrected
During the implementation, the following AI compiler and structural errors were caught and resolved:

* **Missing Import in Services:**
   - *Error:* The generated `src/services/payment.rs` code utilized `Json` to wrap responses in error handling paths (e.g. returning `ACCEPTED` responses) but did not import `axum::Json`.
   - *Resolution:* Added `use axum::Json;` to the service file.
* **Never Type Fallback Error in Axum Route:**
   - *Error:* The generated payment handler in `src/api/payments.rs` used `impl IntoResponse` as the return type of the Result's `Ok` variant. Because of generic response types and multiple exit points, the compiler raised a `dependency_on_unit_never_type_fallback` type inference error.
   - *Resolution:* Refactored the handler signature to use a concrete return type `Result<(StatusCode, Json<PaymentAttemptResponse>), Response>`. This explicitly declared the types to the compiler, eliminating the fallback warning/error.
