---
area: Pricing
stage: implemented
---

# An order's total is the sum of its items

Used by enocone's own suite: linked tables, orders and their line items.

## Then

1. **Each order is sent with its items, and its total matches**

   Table: orders

   | order | customer  | total? |
   |-------|-----------|-------:|
   | A     | new       |  52.50 |
   | B     | returning |  20.00 |

   Table: items

   | orders | item | qty | price |
   |--------|------|----:|------:|
   | A      | book |   1 | 40.00 |
   | A      | pen  |   5 |  2.50 |
   | B      | pen  |   8 |  2.50 |

   ```http
   POST: "{{env.API}}/orders"
   json: { customer: "{{row.customer}}", items: "{{row.items}}" }
   expect:
     json:
       $.total: "{{row.total}}"
   ```
