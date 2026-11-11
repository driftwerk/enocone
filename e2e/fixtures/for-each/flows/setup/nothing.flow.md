---
area: Setup
stage: implemented
---

# Tidy up when there is nothing to tidy

Used by enocone's own suite.

## Given

1. **The group has resources**
   ```http
   POST: "{{env.API}}/resources/nothing{{run.short_id}}"
   json: { name: keep }
   expect: { status: 201 }
   ```

## When

2. **The ids of the demo resources are captured**
   ```http
   GET: "{{env.API}}/resources/nothing{{run.short_id}}"
   capture: { old_ids: "$.results[?@.name == 'demo'].id" }
   ```

3. **Each old demo resource is deleted**

   For each: {{old_ids}}

   ```http
   DELETE: "{{env.API}}/resources/nothing{{run.short_id}}/{{item}}"
   expect: { status: 204 }
   ```

## Then

4. **Only the resource to keep remains**
   ```http
   GET: "{{env.API}}/resources/nothing{{run.short_id}}"
   expect:
     json:
       $.results[*].name: keep
   ```
