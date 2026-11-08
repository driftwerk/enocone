---
id: chart
area: Fixture
---

# Three steps, one of which fails

Used by enocone's own suite to check that charts take their steps' colours.

```flowchart
flowchart LR
  1[Order placed] --> 2[Payment taken] --> 3[Receipt sent]
  2 -.->|declined| retry[Ask for another card]
```

## When

1. **An order is placed**
   ```exec
   # Place the order (always succeeds)
   run: "true"
   ```

2. **The payment is taken**
   ```exec
   # Take the payment (the card is declined)
   run: |
     echo "card declined" >&2; exit 1
   ```

## Then

3. **A receipt is sent**
   ```exec
   # Send the receipt
   run: "true"
   ```
