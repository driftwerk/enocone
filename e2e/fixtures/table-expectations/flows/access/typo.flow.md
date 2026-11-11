---
area: Access
stage: implemented
---

# Who may read the reports, with a typo in one row

Used by enocone's own suite: one row's status cell isn't a status.

## Then

1. **Each kind of user gets their own answer from the reports**

   | who      | status?   |
   |----------|-----------|
   | admin    | 200       |
   | member   | forbidden |
   | stranger | 404       |

   ```http
   GET: "{{env.API}}/reports/{{row.who}}"
   expect:
     status: "{{row.status}}"
   ```
