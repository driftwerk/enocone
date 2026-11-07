---
area: Quotes & payments
tags: [revenue, client-facing, smoke]
owner: quotes-team
priority: critical
---

# Client accepts and pays a conveyancing quote from the emailed link

Most sale instructions start when the client accepts their quote by link.
If acceptance or payment silently fails, the firm loses the instruction and
the client is left unsure whether they paid.

## Given

1. **Harbour Estates refers a sale lead for 14 Elm Road, Bristol, with client Jane Patel**
   ```http
   POST: "{{env.API}}/leads"
   auth: agency-api-key
   json:
     transaction: sale
     property: { address: "14 Elm Road, Bristol", postcode: "BS3 1AA" }
     clients: [{ name: "Jane Patel", email: "jane+{{run.short_id}}@example.test" }]
   expect: { status: 201, json: { $.status: open } }
   capture: { lead_id: $.id }
   ```

2. **The lead has a quote: legal fee £950 + VAT and searches £228, total £1,368.00**
   ```http
   POST: "{{env.API}}/quotes"
   auth: case-handler
   json:
     lead_id: "{{lead_id}}"
     fees:
       - { name: "Legal fee", amount: 95000, vat: true }
       - { name: "Searches pack", amount: 22800 }
   expect: { status: 201, json: { $.total: 136800 } }
   capture: { quote_id: $.id, accept_token: $.accept_token }
   ```

## When

3. **Jane opens the acceptance link from her email and accepts the terms**
   ```browser
   - goto: /public/quotes/{{quote_id}}/accept/{{accept_token}}
   - expect: { heading: Your conveyancing quote }
   - check: { label: I accept the terms of engagement }
   - click: Accept quote
   ```

4. **Jane pays £1,368.00 by card**
   ```browser
   - fill: { label: Card number, value: "4242 4242 4242 4242" }
   - click: Pay £1,368.00
   ```

## Then

5. **Jane sees the payment confirmation with the amount paid**
   ```expect
   text: "£1,368.00"
   within: { role: region, name: Payment confirmed }
   ```

6. **The quote is recorded as paid**
   ```http
   GET: "{{env.API}}/quotes/{{quote_id}}/payment-status"
   auth: case-handler
   eventually: 5s
   expect: { json: { $.status: paid } }
   ```

7. **The lead shows the “Quote accepted” milestone**
   ```browser
   as: case-handler
   steps:
     - goto: /lead/{{lead_id}}/milestones
     - expect: { role: checkbox, name: Quote accepted, checked: true }
   ```

## Cleanup

- Close the test lead
  ```http
  POST: "{{env.API}}/leads/{{lead_id}}/close"
  auth: agency-api-key
  ```
