---
area: Fixture
stage: approved
---

# Client accepts and pays a quote

Used by enocone's own suite to check that storyboards draw the journey.

```storyboard
- { from: quote, to: pay, step: 2, on: { click: Accept quote } }
- { from: pay, to: done, step: 3, on: { click: Pay } }
```

## Given

1. **The client opens the quote**

## When

2. **The client accepts the quote**

3. **The client pays by card**
