---
area: Email
tags: [email]
owner: enocone
priority: high
stage: implemented
approved_by: Kingsley Hendrickse (2026-11-06)
---

# A flow follows the link in an email the product sent

Many journeys start in an inbox: a quote to accept, a password to reset, an
invitation. Test emails go to a sandbox rather than real people. A flow reads
the latest one for an address and follows its link, just as the client would.

## Rules

- An ```email block waits for the latest message to an address, checks its
  subject and body, and captures links or text:
  `capture: { accept_link: { link: "/accept/" } }`.
- Mailboxes are configured per project under `[email]`: Postmark (a sandbox
  server's API) or Mailpit/Mailhog for local stacks.
- It waits up to `within` (default 30s) for the message to arrive.
- Only messages sent after the flow started count, so earlier runs can't
  satisfy it.

## Given

1. **The demo app sends Jane a quote email when a quote is created**
   ```exec
   run: |
     cp -R "{{env.FIXTURES}}/email" "{{run.tmp}}/p" && "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" validate
   expect: { stderr: "2 flows OK" }
   ```

## When

2. **The flow waits for Jane's email and captures the acceptance link**
   ```exec
   run: |
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" run --id quote-email --out "{{run.tmp}}/out" && cat "{{run.tmp}}/out/results.json"
   expect:
     json:
       $.flows[0].attempts[0].steps[1].status: passed
       $.flows[0].attempts[0].steps[1].records[0].type: email
       $.flows[0].attempts[0].steps[1].records[0].mailbox: postmark
       $.flows[0].attempts[0].steps[1].records[0].subject: { contains: Your conveyancing quote }
       $.flows[0].attempts[0].steps[1].captured[0][0]: accept_link
       $.flows[0].attempts[0].steps[1].captured[0][1]: { matches: "^http://127.0.0.1:4010/public/quotes/q_\\w+/accept/tk_\\w+$" }
   ```

## Then

3. **Opening the captured link shows Jane's quote**
   ```exec
   run: |
     cat "{{run.tmp}}/out/results.json"
   expect:
     json:
       $.flows[0].attempts[0].steps[2].status: passed
       $.flows[0].attempts[0].steps[2].page_url: { contains: /accept/ }
   ```
   ```browser
   - goto: "file://{{run.tmp}}/out/index.html#flow=quote-email&step=jane-receives-the-quote-email"
   - expect: "waits for email to jane+"
   - screenshot
   ```

4. **Waiting for an email that never comes fails with what the mailbox did contain**
   ```exec
   run: |
     "{{env.ENOCONE_BIN}}" --project "{{run.tmp}}/p" run --id wrong-subject --out "{{run.tmp}}/missing"
   expect:
     exit: 1
     stderr: { matches: "no email to sam\\+\\w+@example.test with a subject containing “Your invoice” arrived within 2s; the mailbox has 1 message for that address: .*“Your conveyancing quote: £500.00”" }
   ```
