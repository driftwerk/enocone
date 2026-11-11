---
area: Access
stage: implemented
---

# Who may call the reports

Used by enocone's own suite: allowed and refused callers, as two tables.

## Then

1. **Allowed callers get the reports, and refused callers don't**

   Table: allowed

   | who   | status? |
   |-------|--------:|
   | admin |     200 |

   Table: refused

   | who      | status? |
   |----------|--------:|
   | member   |     403 |
   | stranger |     404 |

   ```http
   GET: "{{env.API}}/reports/{{row.who}}"
   expect:
     status: "{{row.status}}"
   ```
