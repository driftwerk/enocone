---
area: Fixture
---

# Client pays for an accepted quote

Used by enocone's own suite to check that the built screen sits beside its mockup.

## When

1. **The client opens the payment page**
   ```screen
   screen: pay
   title: Payment page
   ```
   ```browser
   - goto: "{{env.WEB}}/public/quotes/q-demo/pay"
   - expect: { role: heading, name: Pay for your quote }
   - screenshot
   ```
