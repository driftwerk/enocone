---
area: Access
stage: implemented
---

# Who may read the reports

Used by enocone's own suite: one table states the whole access rule.

## Then

1. **Each kind of user gets their own answer from the reports**

   | who      | status?  |
   |----------|----------|
   | admin    | 200, 201 |
   | member   | 403      |
   | stranger | 404      |

   ```http
   GET: "{{env.API}}/reports/{{row.who}}"
   expect:
     status: "{{row.status}}"
   ```

2. **A command's exit code can come from the row too**

   | ends with | exit? |
   |----------:|------:|
   |         0 |     0 |
   |         3 |  1, 3 |

   ```exec
   # Exit with the row's code
   run: "exit {{row.ends_with}}"
   expect:
     exit: "{{row.exit}}"
   ```
