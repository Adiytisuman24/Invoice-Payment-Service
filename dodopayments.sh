#!/usr/bin/env bash
set -e

# ==============================================================================
#  DodoPayments — Unified Service Runner & Test Suite
# ==============================================================================

CYAN='\033[0;36m'
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m' # No Color

banner() {
  clear 2>/dev/null || true
  echo -e "${CYAN}${BOLD}"
  echo "  ██████╗  ██████╗ ██████╗  ██████╗ ██████╗  █████╗ ██╗   ██╗███╗   ███╗███████╗███╗   ██╗████████╗███████╗"
  echo "  ██╔══██╗██╔═══██╗██╔══██╗██╔═══██╗██╔══██╗██╔══██╗╚██╗ ██╔╝████╗ ████║██╔════╝████╗  ██║╚══██╔══╝██╔════╝"
  echo "  ██║  ██║██║   ██║██║  ██║██║   ██║██████╔╝███████║ ╚████╔╝ ██╔████╔██║█████╗  ██╔██╗ ██║   ██║   ███████╗"
  echo "  ██║  ██║██║   ██║██║  ██║██║   ██║██╔═══╝ ██╔══██║  ╚██╔╝  ██║╚██╔╝██║██╔══╝  ██║╚██╗██║   ██║   ╚════██║"
  echo "  ██████╔╝╚██████╔╝██████╔╝╚██████╔╝██║     ██║  ██║   ██║   ██║ ╚═╝ ██║███████╗██║ ╚████║   ██║   ███████║"
  echo "  ╚═════╝  ╚═════╝ ╚═════╝  ╚═════╝ ╚═╝     ╚═╝  ╚═╝   ╚═╝   ╚═╝     ╚═╝╚══════╝╚═╝  ╚═══╝   ╚═╝   ╚══════╝"
  echo -e "${NC}"
  echo -e "      ${BOLD}Rust + Axum + PostgreSQL + Mock PSP + Asynchronous Webhook Delivery${NC}"
  echo -e "      ──────────────────────────────────────────────────────────────────────────"
  echo ""
}

start_services() {
  echo -e "${YELLOW}▶ Starting all Docker services (PostgreSQL, Invoice API, Mock PSP)...${NC}"
  docker compose up --build -d
  echo ""
  echo -e "${YELLOW}▶ Waiting for services to be ready...${NC}"
  for i in $(seq 1 30); do
    if curl -sf http://localhost:8080/v1/customers -H "Authorization: Bearer dodo_test_seed_key_abc123" > /dev/null 2>&1; then
      echo -e "${GREEN}✓ All services are UP and listening!${NC}"
      echo -e "   • ${BOLD}Invoice API${NC}   → http://localhost:8080"
      echo -e "   • ${BOLD}Mock PSP${NC}      → http://localhost:8081"
      echo -e "   • ${BOLD}PostgreSQL${NC}    → localhost:5432 (dodo)"
      return 0
    fi
    sleep 1
  done
  echo -e "${RED}✗ Services failed to start in time. Check logs with: docker compose logs${NC}"
  exit 1
}

run_smoke_test() {
  echo -e "\n${CYAN}${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
  echo -e "  ${BOLD}RUNNING COMPLETE END-TO-END VERIFICATION TEST SUITE${NC}"
  echo -e "${CYAN}${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}\n"

  AUTH="Authorization: Bearer dodo_test_seed_key_abc123"
  PASS=0
  FAIL=0

  check() {
    local label="$1"
    local cond="$2"
    if [ "$cond" = "1" ]; then
      echo -e "  ${GREEN}✓${NC} $label"
      PASS=$((PASS+1))
    else
      echo -e "  ${RED}✗${NC} $label"
      FAIL=$((FAIL+1))
    fi
  }

  echo -e "${YELLOW}[1/8] Authentication & Customer Creation${NC}"
  # Test missing auth (expect 401)
  AUTH_CODE=$(curl -s -o /dev/null -w "%{http_code}" http://localhost:8080/v1/customers)
  check "Missing auth header returns HTTP 401" "$([ "$AUTH_CODE" = "401" ] && echo 1 || echo 0)"

  # Create customer
  CUS=$(curl -sf -X POST http://localhost:8080/v1/customers \
    -H "$AUTH" -H "Content-Type: application/json" \
    -d '{"name":"Alice Smith","email":"alice@example.com"}')
  CID=$(echo "$CUS" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
  check "Customer created ($CID)" "$([ -n "$CID" ] && echo 1 || echo 0)"

  echo -e "\n${YELLOW}[2/8] Invoice Creation ($1,025.00)${NC}"
  INV=$(curl -sf -X POST http://localhost:8080/v1/invoices \
    -H "$AUTH" -H "Content-Type: application/json" \
    -d "{\"customer_id\":\"$CID\",\"due_date\":\"2026-12-31\",\"items\":[{\"description\":\"Engineering consulting\",\"quantity\":2,\"unit_amount_cents\":50000},{\"description\":\"Hosting fees\",\"quantity\":1,\"unit_amount_cents\":2500}]}")
  IID=$(echo "$INV" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
  TOTAL=$(echo "$INV" | python3 -c "import sys,json; print(json.load(sys.stdin)['total_cents'])")
  check "Invoice created ($IID)" "$([ -n "$IID" ] && echo 1 || echo 0)"
  check "Server calculated total_cents = 102500" "$([ "$TOTAL" = "102500" ] && echo 1 || echo 0)"

  echo -e "\n${YELLOW}[3/8] Successful Payment (tok_success)${NC}"
  P1=$(curl -sf -X POST "http://localhost:8080/v1/invoices/$IID/pay" \
    -H "$AUTH" -H "Idempotency-Key: pay-key-001" -H "Content-Type: application/json" \
    -d '{"card_token":"tok_success"}')
  P1_ST=$(echo "$P1" | python3 -c "import sys,json; print(json.load(sys.stdin)['status'])")
  P1_ID=$(echo "$P1" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
  check "Payment attempt status = succeeded" "$([ "$P1_ST" = "succeeded" ] && echo 1 || echo 0)"
  
  INV_STATE=$(curl -sf "http://localhost:8080/v1/invoices/$IID" -H "$AUTH" | python3 -c "import sys,json; print(json.load(sys.stdin)['state'])")
  check "Invoice transitioned to state = paid" "$([ "$INV_STATE" = "paid" ] && echo 1 || echo 0)"

  echo -e "\n${YELLOW}[4/8] Idempotency Replay (Same Key + Same Body)${NC}"
  P2=$(curl -sf -X POST "http://localhost:8080/v1/invoices/$IID/pay" \
    -H "$AUTH" -H "Idempotency-Key: pay-key-001" -H "Content-Type: application/json" \
    -d '{"card_token":"tok_success"}')
  P2_ID=$(echo "$P2" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])" 2>/dev/null || echo "")
  check "Replay returned identical attempt ID ($P1_ID)" "$([ "$P1_ID" = "$P2_ID" ] && echo 1 || echo 0)"

  echo -e "\n${YELLOW}[5/8] Idempotency Key Conflict (Same Key + Different Body)${NC}"
  CONF_CODE=$(curl -s -o /dev/null -w "%{http_code}" -X POST "http://localhost:8080/v1/invoices/$IID/pay" \
    -H "$AUTH" -H "Idempotency-Key: pay-key-001" -H "Content-Type: application/json" \
    -d '{"card_token":"tok_card_declined"}')
  check "Key reuse with different body rejected with HTTP 409 Conflict" "$([ "$CONF_CODE" = "409" ] && echo 1 || echo 0)"

  echo -e "\n${YELLOW}[6/8] Payment Decline (tok_card_declined)${NC}"
  INV2=$(curl -sf -X POST http://localhost:8080/v1/invoices \
    -H "$AUTH" -H "Content-Type: application/json" \
    -d "{\"customer_id\":\"$CID\",\"due_date\":\"2026-12-31\",\"items\":[{\"description\":\"Monthly SaaS\",\"quantity\":1,\"unit_amount_cents\":2900}]}")
  IID2=$(echo "$INV2" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
  DEC=$(curl -sf -X POST "http://localhost:8080/v1/invoices/$IID2/pay" \
    -H "$AUTH" -H "Idempotency-Key: pay-key-002" -H "Content-Type: application/json" \
    -d '{"card_token":"tok_card_declined"}')
  DEC_ST=$(echo "$DEC" | python3 -c "import sys,json; print(json.load(sys.stdin)['status'])")
  DEC_CODE=$(echo "$DEC" | python3 -c "import sys,json; print(json.load(sys.stdin)['failure_code'])")
  INV2_STATE=$(curl -sf "http://localhost:8080/v1/invoices/$IID2" -H "$AUTH" | python3 -c "import sys,json; print(json.load(sys.stdin)['state'])")
  check "Attempt recorded as failed (failure_code = $DEC_CODE)" "$([ "$DEC_ST" = "failed" ] && [ "$DEC_CODE" = "card_declined" ] && echo 1 || echo 0)"
  check "Invoice remains OPEN for retry" "$([ "$INV2_STATE" = "open" ] && echo 1 || echo 0)"

  echo -e "\n${YELLOW}[7/8] PSP Timeout (tok_timeout — 5s client timeout vs 30s PSP sleep)${NC}"
  INV3=$(curl -sf -X POST http://localhost:8080/v1/invoices \
    -H "$AUTH" -H "Content-Type: application/json" \
    -d "{\"customer_id\":\"$CID\",\"due_date\":\"2026-12-31\",\"items\":[{\"description\":\"Server cluster\",\"quantity\":1,\"unit_amount_cents\":50000}]}")
  IID3=$(echo "$INV3" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
  
  START=$SECONDS
  TOUT=$(curl -s -w "\n__STATUS:%{http_code}" -X POST "http://localhost:8080/v1/invoices/$IID3/pay" \
    -H "$AUTH" -H "Idempotency-Key: pay-key-003" -H "Content-Type: application/json" \
    -d '{"card_token":"tok_timeout"}')
  ELAPSED=$((SECONDS - START))
  TOUT_BODY=$(echo "$TOUT" | grep -v __STATUS)
  TOUT_CODE=$(echo "$TOUT" | grep __STATUS | cut -d: -f2)
  TOUT_ST=$(echo "$TOUT_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['status'])" 2>/dev/null || echo "")
  
  check "Returns HTTP 202 Accepted" "$([ "$TOUT_CODE" = "202" ] && echo 1 || echo 0)"
  check "Payment attempt left in 'pending' state" "$([ "$TOUT_ST" = "pending" ] && echo 1 || echo 0)"
  check "Client timeout resolved in ≤8s (${ELAPSED}s elapsed)" "$([ "$ELAPSED" -le 8 ] && echo 1 || echo 0)"

  echo -e "\n${YELLOW}[8/8] Webhook Endpoint Registration${NC}"
  WH=$(curl -sf -X POST http://localhost:8080/v1/webhook-endpoints \
    -H "$AUTH" -H "Content-Type: application/json" \
    -d '{"url":"https://httpbin.org/post"}')
  WH_SEC=$(echo "$WH" | python3 -c "import sys,json; print(json.load(sys.stdin)['signing_secret'])")
  check "Webhook registered and secret returned ($WH_SEC)" "$([ -n "$WH_SEC" ] && echo 1 || echo 0)"

  echo -e "\n${CYAN}─────────────────────────────────────────────────────────────────────────────${NC}"
  if [ "$FAIL" = "0" ]; then
    echo -e "  ${GREEN}${BOLD}ALL TESTS PASSED ($PASS/$PASS) ✓${NC}"
  else
    echo -e "  ${RED}${BOLD}$FAIL TEST(S) FAILED ($PASS passed, $FAIL failed)${NC}"
  fi
  echo -e "${CYAN}─────────────────────────────────────────────────────────────────────────────${NC}\n"
}

run_cargo_tests() {
  echo -e "${YELLOW}▶ Running Rust integration test suite against PostgreSQL & Mock PSP...${NC}"
  DATABASE_URL="postgres://postgres:postgres@localhost:5432/dodo" \
  API_URL="http://localhost:8080" \
  cargo test
}

stop_services() {
  echo -e "${YELLOW}▶ Stopping and wiping Docker stack...${NC}"
  docker compose down -v
  echo -e "${GREEN}✓ All services stopped and database volume cleaned.${NC}"
}

view_logs() {
  echo -e "${YELLOW}▶ Streaming logs from Invoice API and Mock PSP (Ctrl+C to stop)...${NC}"
  docker compose logs -f
}

# Main entrypoint
banner

if [ "$1" = "start" ]; then
  start_services
elif [ "$1" = "test" ]; then
  start_services
  run_smoke_test
elif [ "$1" = "cargo-test" ]; then
  run_cargo_tests
elif [ "$1" = "stop" ]; then
  stop_services
elif [ "$1" = "logs" ]; then
  view_logs
else
  # Default action: Start services and run full verification
  start_services
  run_smoke_test
  echo -e "${BOLD}Available commands:${NC}"
  echo -e "  ${CYAN}./dodopayments.sh test${NC}        → Run the end-to-end smoke tests"
  echo -e "  ${CYAN}./dodopayments.sh cargo-test${NC}  → Run Rust integration test suite (concurrency, idempotency, failure modes)"
  echo -e "  ${CYAN}./dodopayments.sh logs${NC}        → Stream live container logs"
  echo -e "  ${CYAN}./dodopayments.sh stop${NC}        → Stop services and reset database"
  echo ""
fi
