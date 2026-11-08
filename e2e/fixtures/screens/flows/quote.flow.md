---
area: Fixture
stage: approved
---

# Client accepts a quote

Used by enocone's own suite to check that mockups sit beside their steps.

## Given

1. **A quote has been sent**
   ```exec
   # Stand-in: the quote was sent
   run: "true"
   ```

## When

2. **The client opens the quote**
   ```screen
   screen: quote
   title: Quote page
   ```

## Then

3. **The quote is accepted**
   ```exec
   # Stand-in: the client accepts
   run: "true"
   ```
