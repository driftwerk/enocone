---
area: Fixture
stage: draft
sample: { quote_id: q-sample }
---

# Client accepts and pays a quote from the new quote page

Used by enocone's own suite to check that browser steps run against the
prototype before the feature exists.

```storyboard
- { from: quote, to: pay, step: 2, on: { click: Accept quote } }
- { from: pay, to: done, step: 3, on: { click: Pay } }
```

## Given

1. **A quote has been sent**
   ```http
   POST: "{{env.API}}/quotes"
   headers: { X-API-Key: agency-key-98765 }
   json: { lead_id: ld-1, fees: [{ amount: 120833, vat: true }] }
   expect: { status: 201 }
   capture: { quote_id: $.id }
   ```

## When

2. **The client opens the new quote page and accepts the quote**
   ```screen
   screen: quote
   path: /quotes/{quote_id}
   ```
   ```browser
   - goto: "/quotes/{{quote_id}}"
   - expect: { heading: "£1,450.00 incl. VAT" }
   - check: { label: I accept the terms of engagement }
   - click: Accept quote
   ```

3. **The client pays by card**
   ```screen
   screen: pay
   path: /quotes/{quote_id}/pay
   ```
   ```browser
   - expect: { heading: Pay for your quote }
   - fill: { label: Card number, value: "4242 4242 4242 4242" }
   - click: Pay £1,450.00
   ```

## Then

4. **The client sees that the payment is confirmed**
   ```screen
   screen: done
   path: /quotes/{quote_id}/done
   ```
   ```browser
   - expect: { heading: Payment confirmed }
   - expect: { url: /quotes/q-sample/done }
   ```
