---
area: Pricing
stage: implemented
---

# The discount follows the customer and the order total

Used by enocone's own suite: a table of examples with one wrong row.

## Then

1. **The discount depends on the customer and the order total**

   | customer  | total | discount? |
   |-----------|------:|----------:|
   | new       |    40 |        0% |
   | new       |   120 |        5% |
   | returning |    40 |        5% |
   | returning |   120 |       15% |

   ```http
   POST: "{{env.API}}/discount"
   json: { customer: "{{row.customer}}", total: "{{row.total}}" }
   expect:
     json:
       $.discount: "{{row.discount}}"
   ```
