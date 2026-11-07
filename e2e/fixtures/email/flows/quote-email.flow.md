---
area: Fixture
---

# Jane accepts her quote from the email link

Used by enocone's own suite to check that email blocks find the message and capture its link.

## Given

1. **A quote is created for Jane**
   ```http
   POST: "{{env.API}}/quotes"
   headers: { X-API-Key: agency-key-98765 }
   json: { lead_id: ld-1, client_email: "jane+{{run.short_id}}@example.test", fees: [{ amount: 120834, vat: true }] }
   expect: { status: 201 }
   capture: { quote_id: $.id }
   ```

## When

2. **Jane receives the quote email**
   ```email
   to: "jane+{{run.short_id}}@example.test"
   subject: Your conveyancing quote
   within: 10s
   expect: { body: "£1,450.00" }
   capture: { accept_link: { link: "/accept/" } }
   ```

## Then

3. **The link opens Jane's quote**
   ```browser
   - goto: "{{accept_link}}"
   - expect: { heading: Your conveyancing quote }
   - expect: "Total: £1,450.00"
   ```
