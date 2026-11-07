---
area: Fixture
---

# Waiting for an email that never comes

Used by enocone's own suite to check the failure message lists what the mailbox did contain.

## Given

1. **A quote is created for Sam**
   ```http
   POST: "{{env.API}}/quotes"
   headers: { X-API-Key: agency-key-98765 }
   json: { lead_id: ld-2, client_email: "sam+{{run.short_id}}@example.test", fees: [{ amount: 50000 }] }
   expect: { status: 201 }
   ```

## When

2. **Sam receives an invoice email**
   ```email
   to: "sam+{{run.short_id}}@example.test"
   subject: Your invoice
   within: 2s
   ```
