---
stage: implemented
---

# A block names a column the table doesn't have

Used by enocone's own suite: validation of tables of examples.

## Then

1. **The discount is checked**

   | customer | total | discount? |
   |----------|------:|----------:|
   | new      |    40 |        0% |

   ```http
   POST: "{{env.API}}/discount"
   json: { customer: "{{row.customer}}", total: "{{row.totl}}" }
   expect:
     json:
       $.discount: "{{row.discount}}"
   ```
