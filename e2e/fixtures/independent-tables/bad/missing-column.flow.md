---
area: Access
stage: implemented
---

# One table lacks a column the block uses

## Then

1. **Allowed callers get the reports, and refused callers don't**

   Table: allowed

   | who   | status? |
   |-------|--------:|
   | admin |     200 |

   Table: refused

   | caller | status? |
   |--------|--------:|
   | member |     403 |

   ```http
   GET: "{{env.API}}/reports/{{row.who}}"
   expect:
     status: "{{row.status}}"
   ```
