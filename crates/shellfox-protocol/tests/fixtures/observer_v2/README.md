# Observer-v2 golden bodies

Canonical profile `observer-v2`, version 2. These are reviewed contract examples, not production captures. V1 fixtures are untouched.

The normal session example derives field values from `crates/shellfox-client/tests/fixtures/manager-snapshot-success.json`. The raw-source example intentionally has unknown creation facts and not-applicable shell/profile facts. The unavailable entry remains in the complete authorised inventory. Source kinds are metadata, never process cleanup authority.

Recovery bytes `GBtj` illustrate CAN + RIS only, not a captured full formatter result or a FullState claim. Live bytes `QQBC` encode A-NUL-B. Source end and independently evidenced original-process exit are separate events.

Protocol models live in `shellfox_protocol::observer_v2`. New paths are exactly POST /v2/discover, /v2/pair, /v2/request, /v2/events and /v2/disconnect. Adding types does not enable a production route. The backend must bind actual authenticated transport peer + client/owner/connection, not trust JSON identifiers. See the protocol decode_request/check_connection helpers.
